use crate::{
    engine::{Event, Instance, Item, Shared, lock},
    model::*,
    storage,
};
use chrono::{DateTime, FixedOffset, Local, SecondsFormat, Timelike};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Stamp {
    pub mono: Duration,
    pub wall: SystemTime,
}
pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Stamp;
    fn local(&self, wall: SystemTime) -> DateTime<FixedOffset>;
}
pub(crate) struct RealClock {
    start: Instant,
}
impl RealClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}
impl Clock for RealClock {
    fn now(&self) -> Stamp {
        Stamp {
            mono: self.start.elapsed(),
            wall: SystemTime::now(),
        }
    }
    fn local(&self, wall: SystemTime) -> DateTime<FixedOffset> {
        DateTime::<Local>::from(wall).fixed_offset()
    }
}
pub(crate) trait Output: Send {
    fn write(&mut self, path: &str, line: &str) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
    fn close_expired(&mut self, _now: i64) -> io::Result<()> {
        Ok(())
    }
    fn close_all(&mut self) -> io::Result<()> {
        self.flush()
    }
}
struct Writer {
    buffer: BufWriter<File>,
    used: u64,
    dirty: bool,
    path: PathBuf,
    hour_end: i64,
}
pub(crate) struct FileOutput {
    pub log_root: PathBuf,
    pub run_id: String,
    _lease: File,
    root_identity: fs::Metadata,
    writers: HashMap<String, Writer>,
    limit: usize,
    capacity: usize,
    tick: u64,
    segment: u64,
}
impl FileOutput {
    pub fn new(config: &Config) -> io::Result<Self> {
        static RUN: AtomicU64 = AtomicU64::new(0);
        let log_root = config.directory.join("rlogger");
        storage::initialize(&log_root)?;
        let log_root = fs::canonicalize(log_root)?;
        let _gate = storage::gate(&log_root)?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let (run_id, lease) = loop {
            let id = format!(
                "{stamp}-{}-{}",
                std::process::id(),
                RUN.fetch_add(1, Ordering::Relaxed)
            );
            match storage::open_beneath(
                &log_root,
                &PathBuf::from(storage::TECH).join(format!("{id}.lock")),
                true,
            ) {
                Ok(f) => {
                    f.lock()?;
                    break (id, f);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        Ok(Self {
            root_identity: fs::metadata(&log_root)?,
            log_root,
            run_id,
            _lease: lease,
            writers: HashMap::new(),
            limit: config.max_writers,
            capacity: config.writer_buffer_bytes,
            tick: 0,
            segment: 0,
        })
    }
    fn close_writer(&mut self, key: &str) -> io::Result<()> {
        let Some(mut writer) = self.writers.remove(key) else {
            return Ok(());
        };
        let result = writer.buffer.flush();
        let (file, _) = writer.buffer.into_parts();
        drop(file);
        result?;
        let _gate = storage::gate(&self.log_root)?;
        self.validate_root()?;
        let target = writer.path.with_file_name(
            writer
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .replace(".active.log", ".log"),
        );
        storage::rename_beneath(
            &self.log_root,
            writer.path.strip_prefix(&self.log_root).unwrap(),
            target.strip_prefix(&self.log_root).unwrap(),
        )
    }
    fn validate_root(&self) -> io::Result<()> {
        let current = fs::symlink_metadata(&self.log_root)?;
        if current.file_type().is_symlink() || !storage::same_file(&self.root_identity, &current) {
            return Err(io::Error::other("RLOGGER root replaced"));
        }
        Ok(())
    }
}
impl Output for FileOutput {
    fn write(&mut self, path: &str, line: &str) -> io::Result<()> {
        self.tick += 1;
        if !self.writers.contains_key(path) {
            if self.writers.len() >= self.limit {
                let oldest = self
                    .writers
                    .iter()
                    .min_by_key(|(_, w)| w.used)
                    .map(|(p, _)| p.clone())
                    .expect("nonempty writer cache");
                self.close_writer(&oldest)?;
            }
            self.segment = self
                .segment
                .checked_add(1)
                .ok_or_else(|| io::Error::other("segment exhausted"))?;
            // Logical path is day/instance/destination-HH-h<UTC start>.
            let (base, hour) = path
                .rsplit_once("-h")
                .ok_or_else(|| io::Error::other("invalid hour key"))?;
            let hour_start = hour.parse::<i64>().map_err(io::Error::other)?;
            let full = self.log_root.join(format!(
                "{base}-{}-h{hour}-s{}.active.log",
                self.run_id, self.segment
            ));
            let _gate = storage::gate(&self.log_root)?;
            self.validate_root()?;
            let file = storage::open_beneath(
                &self.log_root,
                full.strip_prefix(&self.log_root).unwrap(),
                true,
            )?;
            self.writers.insert(
                path.to_owned(),
                Writer {
                    buffer: BufWriter::with_capacity(self.capacity, file),
                    used: self.tick,
                    dirty: false,
                    path: full,
                    hour_end: hour_start
                        .checked_add(3600)
                        .ok_or_else(|| io::Error::other("hour overflow"))?,
                },
            );
        }
        let writer = self.writers.get_mut(path).expect("writer opened");
        writer.used = self.tick;
        writer.dirty = true;
        writer.buffer.write_all(line.as_bytes())
    }
    fn flush(&mut self) -> io::Result<()> {
        for writer in self.writers.values_mut() {
            if writer.dirty {
                writer.buffer.flush()?;
                writer.dirty = false;
            }
        }
        Ok(())
    }
    fn close_expired(&mut self, now: i64) -> io::Result<()> {
        let keys: Vec<_> = self
            .writers
            .iter()
            .filter(|(_, w)| w.hour_end <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            self.close_writer(&key)?;
        }
        Ok(())
    }
    fn close_all(&mut self) -> io::Result<()> {
        for key in self.writers.keys().cloned().collect::<Vec<_>>() {
            self.close_writer(&key)?;
        }
        Ok(())
    }
}
impl Drop for FileOutput {
    fn drop(&mut self) {
        // Never replay a possibly partial failed write through BufWriter::drop.
        for (_, writer) in self.writers.drain() {
            let _ = writer.buffer.into_parts();
        }
    }
}
struct Group {
    first: Event,
    last: Stamp,
    last_seq: u64,
    count: u64,
    local: DateTime<FixedOffset>,
    path: String,
}
#[derive(Default)]
struct Confirmation {
    occurrences: u64,
    lines: u64,
    overload: u64,
}
pub(crate) struct Worker {
    shared: Arc<Shared>,
    output: Box<dyn Output>,
    groups: BTreeMap<(usize, String), Group>,
    unconfirmed: BTreeMap<usize, Confirmation>,
    last_flush: Duration,
}
impl Worker {
    pub fn new(shared: Arc<Shared>, output: Box<dyn Output>) -> Self {
        Self {
            shared,
            output,
            groups: BTreeMap::new(),
            unconfirmed: BTreeMap::new(),
            last_flush: Duration::ZERO,
        }
    }
    pub fn run(mut self) {
        if let Err(error) = self.loop_run() {
            self.shared.fail(error.to_string());
        }
    }
    fn loop_run(&mut self) -> io::Result<()> {
        loop {
            let stamp = self.shared.clock.now();
            let now = stamp.mono;
            let wall: DateTime<chrono::Utc> = stamp.wall.into();
            let expired = self
                .groups
                .iter()
                .filter(|(_, g)| {
                    now.saturating_sub(g.first.stamp.mono) >= self.shared.config.group_duration
                        || hour_start(&g.local).saturating_add(3600) <= wall.timestamp()
                })
                .map(|(k, _)| k.clone())
                .collect::<Vec<_>>();
            for key in expired {
                self.finish(&key)?;
            }
            self.output.close_expired(wall.timestamp())?;
            if now.saturating_sub(self.last_flush) >= self.shared.config.flush_interval {
                self.flush_output()?;
                self.last_flush = now;
            }
            let item = {
                let mut s = lock(&self.shared.state);
                if s.health == Health::Failed {
                    return Ok(());
                }
                if s.health == Health::Open {
                    self.shared.summaries(&mut s, false);
                }
                if let Some(item) = s.queue.pop_front() {
                    Some(item)
                } else {
                    let group_wait = self
                        .groups
                        .values()
                        .map(|g| {
                            self.shared
                                .config
                                .group_duration
                                .saturating_sub(now.saturating_sub(g.first.stamp.mono))
                        })
                        .min()
                        .unwrap_or(self.shared.config.overload_interval);
                    let wait = group_wait
                        .min(self.shared.config.flush_interval)
                        .min(self.shared.config.overload_interval);
                    let _guard = self
                        .shared
                        .wake
                        .wait_timeout(s, wait)
                        .unwrap_or_else(|e| e.into_inner());
                    None
                }
            };
            match item {
                None => {}
                Some(Item::Event(event)) => self.event(event)?,
                Some(Item::Summary {
                    instance,
                    stamp,
                    sequence,
                    count,
                }) => {
                    self.finish_instance(instance.id)?;
                    let local = self.shared.clock.local(stamp.wall);
                    let path = path_for(&instance, &instance.config.destinations[0], &local);
                    let latency = self
                        .shared
                        .clock
                        .now()
                        .mono
                        .saturating_sub(stamp.mono)
                        .as_millis();
                    let line = format!(
                        "RLOG/1 {} WARN [LATENCY — {latency}ms] [seq={sequence} instance={} source=logger] OVERLOAD refused={count}\n",
                        local.to_rfc3339_opts(SecondsFormat::Millis, true),
                        instance.config.name
                    );
                    self.output.write(&path, &line)?;
                    let c = self.unconfirmed.entry(instance.id).or_default();
                    c.lines += 1;
                    c.overload += count;
                    lock(&self.shared.state).instances[instance.id].summary_queued = false;
                }
                Some(Item::Barrier(id)) => {
                    self.finish_all()?;
                    self.flush_output()?;
                    lock(&self.shared.state).completed_barrier = id;
                    self.shared.wake.notify_all();
                }
                Some(Item::Shutdown) => {
                    // A summary already queued at closure may have left an additional balance.
                    let more = {
                        let mut s = lock(&self.shared.state);
                        self.shared.summaries(&mut s, true);
                        if !s.queue.is_empty() {
                            s.queue.push_back(Item::Shutdown);
                            true
                        } else {
                            false
                        }
                    };
                    if more {
                        continue;
                    }
                    self.finish_all()?;
                    self.flush_output()?;
                    self.output.close_all()?;
                    lock(&self.shared.state).health = Health::Closed;
                    self.shared.wake.notify_all();
                    return Ok(());
                }
            }
        }
    }
    fn event(&mut self, event: Event) -> io::Result<()> {
        let local = self.shared.clock.local(event.stamp.wall);
        let path = path_for(&event.instance, &event.destination, &local);
        let key = (event.instance.id, event.destination.clone());
        let can_merge = self.groups.get(&key).is_some_and(|g| {
            g.path == path
                && g.local.offset() == local.offset()
                && g.first.epoch == event.epoch
                && g.first.level == event.level
                && g.first.source == event.source
                && g.first.fields == event.fields
                && g.first.message == event.message
                && g.first.thread == event.thread
                && event.stamp.mono.saturating_sub(g.first.stamp.mono)
                    < self.shared.config.group_duration
        });
        if can_merge {
            let g = self.groups.get_mut(&key).expect("existing group");
            g.last = event.stamp;
            g.last_seq = event.sequence;
            g.count += 1;
        } else {
            self.finish(&key)?;
            self.groups.insert(
                key,
                Group {
                    last: event.stamp,
                    last_seq: event.sequence,
                    count: 1,
                    first: event,
                    local,
                    path,
                },
            );
        }
        Ok(())
    }
    fn finish(&mut self, key: &(usize, String)) -> io::Result<()> {
        let Some(group) = self.groups.remove(key) else {
            return Ok(());
        };
        let event = &group.first;
        let latency = self
            .shared
            .clock
            .now()
            .mono
            .saturating_sub(event.stamp.mono)
            .as_millis();
        let seq = if group.count == 1 {
            format!("seq={}", event.sequence)
        } else {
            format!(
                "seq_first={} seq_last={} count={} last={}",
                event.sequence,
                group.last_seq,
                group.count,
                self.shared
                    .clock
                    .local(group.last.wall)
                    .to_rfc3339_opts(SecondsFormat::Millis, true)
            )
        };
        let mut line = format!(
            "RLOG/1 {} {} [LATENCY — {latency}ms] [{seq} instance={} source=\"{}:{}:{}\"",
            group.local.to_rfc3339_opts(SecondsFormat::Millis, true),
            event.level,
            event.instance.config.name,
            escape(event.source.module),
            escape(event.source.file),
            event.source.line
        );
        if let Some(thread) = &event.thread {
            line.push_str(&format!(" thread=\"{}\"", escape(thread)));
        }
        for (key, value) in &event.fields {
            line.push_str(&format!(" {key}={}", value.render()));
        }
        line.push_str("] ");
        line.push_str(&escape(&event.message));
        if group.count > 1 {
            line.push_str(&format!(" x{}", group.count));
        }
        line.push('\n');
        self.output.write(&group.path, &line)?;
        let c = self.unconfirmed.entry(event.instance.id).or_default();
        c.occurrences += group.count;
        c.lines += 1;
        Ok(())
    }
    fn finish_instance(&mut self, id: usize) -> io::Result<()> {
        let keys = self
            .groups
            .keys()
            .filter(|(i, _)| *i == id)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.finish(&key)?;
        }
        Ok(())
    }
    fn finish_all(&mut self) -> io::Result<()> {
        while let Some(key) = self.groups.keys().next().cloned() {
            self.finish(&key)?;
        }
        Ok(())
    }
    fn flush_output(&mut self) -> io::Result<()> {
        if self.unconfirmed.is_empty() {
            return Ok(());
        }
        self.output.flush()?;
        let mut s = lock(&self.shared.state);
        for (id, c) in std::mem::take(&mut self.unconfirmed) {
            let counters = &mut s.instances[id].counters;
            counters.written += c.occurrences;
            counters.lines += c.lines;
            counters.overload_reported += c.overload;
        }
        Ok(())
    }
}
pub(crate) fn path_for(
    instance: &Instance,
    destination: &str,
    local: &DateTime<FixedOffset>,
) -> String {
    format!(
        "{}/{}/{}-{}-h{}",
        local.format("%Y-%m-%d"),
        instance.config.name,
        destination,
        local.format("%H"),
        hour_start(local)
    )
}
fn hour_start(local: &DateTime<FixedOffset>) -> i64 {
    local.timestamp() - i64::from(local.minute()) * 60 - i64::from(local.second())
}
