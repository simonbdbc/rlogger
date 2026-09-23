use crate::{
    model::*,
    output::{Clock, FileOutput, Output, RealClock, Stamp, Worker},
};
use std::{
    collections::VecDeque,
    fmt::{self, Write},
    sync::{Arc, Condvar, Mutex, MutexGuard, Weak},
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
pub(crate) struct Instance {
    pub config: InstanceConfig,
    pub id: usize,
}
pub(crate) struct InstanceState {
    pub instance: Arc<Instance>,
    pub counters: Counters,
    pub epoch: u64,
    pub overload_pending: u64,
    pub summary_queued: bool,
    pub last_summary: Duration,
}
pub(crate) struct State {
    pub health: Health,
    pub error: Option<String>,
    pub queue: VecDeque<Item>,
    pub instances: Vec<InstanceState>,
    pub sequence: u64,
    pub reserved_events: usize,
    pub reserved_bytes: usize,
    pub context_bytes: usize,
    pub completed_barrier: u64,
    pub next_barrier: u64,
}
pub(crate) struct Shared {
    pub state: Mutex<State>,
    pub wake: Condvar,
    pub config: Config,
    pub clock: Arc<dyn Clock>,
}
impl Shared {
    pub fn report(&self) -> Report {
        let state = lock(&self.state);
        let mut report = Report {
            health: state.health,
            error: state.error.clone(),
            reserved_events: state.reserved_events,
            reserved_bytes: state.reserved_bytes,
            context_bytes: state.context_bytes,
            ..Report::default()
        };
        for i in &state.instances {
            let c = &i.counters;
            let t = &mut report.totals;
            t.accepted += c.accepted;
            t.filtered += c.filtered;
            t.full += c.full;
            t.too_large += c.too_large;
            t.closed += c.closed;
            t.failed += c.failed;
            t.invalid += c.invalid;
            t.unknown_destination += c.unknown_destination;
            t.written += c.written;
            t.lines += c.lines;
            t.overload_reported += c.overload_reported;
            report
                .instances
                .insert(i.instance.config.name.clone(), c.clone());
        }
        report.pending = report.totals.accepted - report.totals.written;
        report
    }
    pub fn fail(&self, error: String) {
        let queue = {
            let mut state = lock(&self.state);
            state.health = Health::Failed;
            state.error = Some(error);
            std::mem::take(&mut state.queue)
        };
        drop(queue);
        self.wake.notify_all();
    }
    fn refusal(state: &mut State, id: usize, reason: Refusal) -> EmitResult {
        let i = &mut state.instances[id];
        match reason {
            Refusal::Full => {
                i.counters.full += 1;
                i.overload_pending += 1;
            }
            Refusal::TooLarge => i.counters.too_large += 1,
            Refusal::Closed => i.counters.closed += 1,
            Refusal::Failed => i.counters.failed += 1,
            Refusal::Invalid => i.counters.invalid += 1,
            Refusal::UnknownDestination => i.counters.unknown_destination += 1,
        };
        // Conservative per-instance continuity break, even when another destination refused.
        i.epoch = i.epoch.wrapping_add(1);
        EmitResult::Refused(reason)
    }
    pub fn summaries(&self, state: &mut State, force: bool) {
        let now = self.clock.now();
        for id in 0..state.instances.len() {
            let i = &mut state.instances[id];
            if i.overload_pending == 0
                || i.summary_queued
                || (!force
                    && now.mono.saturating_sub(i.last_summary) < self.config.overload_interval)
            {
                continue;
            }
            let count = std::mem::take(&mut i.overload_pending);
            i.summary_queued = true;
            i.last_summary = now.mono;
            state.sequence += 1;
            state.queue.push_back(Item::Summary {
                instance: i.instance.clone(),
                stamp: now,
                sequence: state.sequence,
                count,
            });
        }
    }
}
pub(crate) struct Permit {
    shared: Arc<Shared>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut s = lock(&self.shared.state);
        s.reserved_events -= 1;
        s.reserved_bytes -= self.shared.config.max_event_bytes;
    }
}
struct Context {
    fields: Fields,
    bytes: usize,
    shared: Weak<Shared>,
}
impl Drop for Context {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            lock(&shared.state).context_bytes -= self.bytes;
        }
    }
}
pub(crate) struct Event {
    pub instance: Arc<Instance>,
    pub destination: String,
    pub level: Level,
    pub source: Source,
    pub fields: Fields,
    pub message: String,
    pub thread: Option<String>,
    pub sequence: u64,
    pub stamp: Stamp,
    pub epoch: u64,
    pub _permit: Permit,
}
pub(crate) enum Item {
    Event(Event),
    Summary {
        instance: Arc<Instance>,
        stamp: Stamp,
        sequence: u64,
        count: u64,
    },
    Barrier(u64),
    Shutdown,
}

/// Owns one worker. Dropping it drains and joins, potentially waiting on disk I/O.
/// Cloning a [`Logger`] never creates or stops a worker.
pub struct Runtime {
    pub(crate) shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
    control: Mutex<()>,
    log_root: std::path::PathBuf,
    run_id: String,
}
impl Runtime {
    pub fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let output = FileOutput::new(&config)?;
        let log_root = output.log_root.clone();
        let run_id = output.run_id.clone();
        let mut runtime = Self::start(
            config,
            Arc::new(RealClock::new()),
            Box::new(output),
            log_root,
        )?;
        runtime.run_id = run_id;
        Ok(runtime)
    }
    pub(crate) fn start(
        config: Config,
        clock: Arc<dyn Clock>,
        output: Box<dyn Output>,
        run_path: std::path::PathBuf,
    ) -> Result<Self, Error> {
        config.validate()?;
        let queue_capacity = config
            .max_events
            .min(config.max_bytes / config.max_event_bytes)
            .checked_add(config.max_instances)
            .and_then(|n| n.checked_add(2))
            .ok_or(Error::Invalid("queue capacity overflow"))?;
        let mut queue = VecDeque::new();
        queue
            .try_reserve_exact(queue_capacity)
            .map_err(|_| Error::Limit("queue allocation"))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                health: Health::Open,
                error: None,
                queue,
                instances: Vec::new(),
                sequence: 0,
                reserved_events: 0,
                reserved_bytes: 0,
                context_bytes: 0,
                completed_barrier: 0,
                next_barrier: 0,
            }),
            wake: Condvar::new(),
            config,
            clock,
        });
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("ordered-log-writer".into())
            .spawn(move || {
                let failure_shared = worker_shared.clone();
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Worker::new(worker_shared, output).run()
                }));
                if outcome.is_err() {
                    failure_shared.fail("worker panicked; buffered writes are unconfirmed".into());
                }
            })?;
        Ok(Self {
            shared,
            worker: Mutex::new(Some(worker)),
            control: Mutex::new(()),
            log_root: run_path,
            run_id: String::new(),
        })
    }
    pub fn log_root(&self) -> &std::path::Path {
        &self.log_root
    }
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    pub fn instance(&self, mut config: InstanceConfig) -> Result<Logger, Error> {
        if !valid_name(&config.name)
            || config.destinations.is_empty()
            || config.destinations.iter().any(|n| !valid_name(n))
        {
            return Err(Error::Invalid("names must be lowercase ASCII identifiers"));
        }
        let mut names = config.destinations.clone();
        names.sort();
        names.dedup();
        if names.len() != config.destinations.len() {
            return Err(Error::Invalid("duplicate destination"));
        }
        if config.routes.iter().any(|r| {
            r.module_prefix.is_empty()
                || r.module_prefix.len() > 256
                || !config.destinations.contains(&r.destination)
        }) || config.routes.len() > 128
        {
            return Err(Error::Invalid("invalid routing table"));
        }
        let mut prefixes = config
            .routes
            .iter()
            .map(|r| r.module_prefix.as_str())
            .collect::<Vec<_>>();
        prefixes.sort();
        prefixes.dedup();
        if prefixes.len() != config.routes.len() {
            return Err(Error::Invalid("duplicate route"));
        }
        config
            .routes
            .sort_by_key(|r| std::cmp::Reverse(r.module_prefix.len()));
        let mut fields = self.shared.config.context.clone();
        fields.extend(config.context.clone());
        if field_size(&fields)? > self.shared.config.max_event_bytes / 2 {
            return Err(Error::Invalid("instance context exceeds half event budget"));
        }
        config.context = fields;
        let mut s = lock(&self.shared.state);
        match s.health {
            Health::Open => {}
            Health::Failed => return Err(Error::Failed(s.error.clone().unwrap_or_default())),
            _ => return Err(Error::Closed),
        }
        if s.instances.len() >= self.shared.config.max_instances
            || s.instances
                .iter()
                .map(|i| i.instance.config.destinations.len())
                .sum::<usize>()
                + config.destinations.len()
                > self.shared.config.max_destinations
        {
            return Err(Error::Limit("instance/destination registry"));
        }
        if s.instances
            .iter()
            .any(|i| i.instance.config.name == config.name)
        {
            return Err(Error::Invalid("duplicate instance name"));
        }
        let instance = Arc::new(Instance {
            config,
            id: s.instances.len(),
        });
        s.instances.push(InstanceState {
            instance: instance.clone(),
            counters: Counters::default(),
            epoch: 0,
            overload_pending: 0,
            summary_queued: false,
            last_summary: Duration::ZERO,
        });
        Ok(Logger {
            shared: self.shared.clone(),
            instance,
            context: None,
        })
    }
    pub fn stats(&self) -> Report {
        self.shared.report()
    }
    /// A global barrier; file buffers are flushed, but no `fsync` is performed.
    pub fn flush(&self) -> Result<Report, Error> {
        let _control = lock(&self.control);
        let mut s = lock(&self.shared.state);
        match s.health {
            Health::Open => {}
            Health::Failed => return Err(Error::Failed(s.error.clone().unwrap_or_default())),
            _ => return Err(Error::Closed),
        }
        self.shared.summaries(&mut s, true);
        s.next_barrier += 1;
        let barrier = s.next_barrier;
        s.queue.push_back(Item::Barrier(barrier));
        self.shared.wake.notify_one();
        while s.completed_barrier < barrier && s.health != Health::Failed {
            s = self.shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        if s.health == Health::Failed {
            return Err(Error::Failed(s.error.clone().unwrap_or_default()));
        }
        drop(s);
        Ok(self.stats())
    }
    /// Idempotently closes admission, drains, flushes and joins the worker.
    pub fn shutdown(&self) -> Result<Report, ShutdownError> {
        let _control = lock(&self.control);
        {
            let mut s = lock(&self.shared.state);
            if s.health == Health::Open {
                s.health = Health::Closing;
                self.shared.summaries(&mut s, true);
                s.queue.push_back(Item::Shutdown);
                self.shared.wake.notify_one();
            }
        }
        if let Some(worker) = lock(&self.worker).take()
            && worker.join().is_err()
        {
            self.shared.fail("worker join failed".into());
        }
        let report = self.stats();
        if report.health == Health::Failed {
            Err(ShutdownError {
                report: Box::new(report),
            })
        } else {
            Ok(report)
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Send + Sync immutable instance/context handle. Operations snapshot owned data.
#[derive(Clone)]
pub struct Logger {
    pub(crate) shared: Arc<Shared>,
    pub(crate) instance: Arc<Instance>,
    context: Option<Arc<Context>>,
}
impl Logger {
    /// Consistent counters for this instance without allocating a report map.
    pub fn counters(&self) -> Counters {
        lock(&self.shared.state).instances[self.instance.id]
            .counters
            .clone()
    }
    pub fn enabled(&self, level: Level) -> bool {
        level >= self.instance.config.level
    }
    pub fn filtered(&self) -> EmitResult {
        lock(&self.shared.state).instances[self.instance.id]
            .counters
            .filtered += 1;
        EmitResult::Filtered
    }
    pub fn stats(&self) -> Report {
        self.shared.report()
    }
    /// Application < instance < operation < event. Clones share the same budget.
    pub fn with_context(&self, fields: &Fields) -> Result<Self, Error> {
        let extra = field_size(fields)?;
        if extra > self.shared.config.max_event_bytes / 2 {
            return Err(Error::Invalid("operation context too large"));
        }
        // Reserve before copying; replacement temporarily owns both contexts.
        let bytes = self.shared.config.max_event_bytes;
        {
            let mut s = lock(&self.shared.state);
            if s.health != Health::Open {
                return Err(Error::Closed);
            }
            if bytes
                > self
                    .shared
                    .config
                    .max_context_bytes
                    .saturating_sub(s.context_bytes)
            {
                return Err(Error::Limit("shared context budget"));
            }
            s.context_bytes += bytes;
        }
        let mut context = Context {
            fields: Fields::new(),
            bytes,
            shared: Arc::downgrade(&self.shared),
        };
        context.fields = self.context.as_ref().map_or_else(
            || self.instance.config.context.clone(),
            |c| c.fields.clone(),
        );
        context.fields.extend(fields.clone());
        if field_size(&context.fields)? > bytes / 2 {
            return Err(Error::Invalid("effective context too large"));
        }
        Ok(Self {
            shared: self.shared.clone(),
            instance: self.instance.clone(),
            context: Some(Arc::new(context)),
        })
    }
    pub fn emit(
        &self,
        level: Level,
        source: Source,
        destination: Option<&str>,
        fields: &Fields,
        message: &str,
    ) -> EmitResult {
        self.emit_args(
            level,
            source,
            destination,
            fields,
            format_args!("{message}"),
        )
    }
    pub(crate) fn refuse(&self, reason: Refusal) -> EmitResult {
        let result = Shared::refusal(&mut lock(&self.shared.state), self.instance.id, reason);
        self.shared.wake.notify_one();
        result
    }
    pub fn emit_args(
        &self,
        level: Level,
        source: Source,
        destination: Option<&str>,
        fields: &Fields,
        args: fmt::Arguments<'_>,
    ) -> EmitResult {
        if !self.enabled(level) {
            return self.filtered();
        }
        if source.module.len() > 512 || source.file.len() > 1024 {
            return self.refuse(Refusal::TooLarge);
        }
        let dest = if let Some(d) = destination {
            if !self.instance.config.destinations.iter().any(|x| x == d) {
                return self.refuse(Refusal::UnknownDestination);
            }
            d
        } else if let Some(route) = self.instance.config.routes.iter().find(|r| {
            source.module == r.module_prefix
                || source
                    .module
                    .strip_prefix(&r.module_prefix)
                    .is_some_and(|s| s.starts_with("::"))
        }) {
            &route.destination
        } else {
            let krate = source.module.split("::").next().unwrap_or("");
            self.instance
                .config
                .destinations
                .iter()
                .find(|d| d.as_str() == krate)
                .unwrap_or(&self.instance.config.destinations[0])
        };
        let fsize = match field_size(fields) {
            Ok(v) => v,
            Err(_) => return self.refuse(Refusal::Invalid),
        };
        if fsize > self.shared.config.max_event_bytes {
            return self.refuse(Refusal::TooLarge);
        }
        {
            let mut s = lock(&self.shared.state);
            let reason = match s.health {
                Health::Failed => Some(Refusal::Failed),
                Health::Open => None,
                _ => Some(Refusal::Closed),
            };
            if let Some(r) = reason {
                return Shared::refusal(&mut s, self.instance.id, r);
            }
            if s.reserved_events >= self.shared.config.max_events
                || self.shared.config.max_event_bytes
                    > self
                        .shared
                        .config
                        .max_bytes
                        .saturating_sub(s.reserved_bytes)
            {
                let result = Shared::refusal(&mut s, self.instance.id, Refusal::Full);
                self.shared.wake.notify_one();
                return result;
            }
            s.reserved_events += 1;
            s.reserved_bytes += self.shared.config.max_event_bytes;
        }
        let permit = Permit {
            shared: self.shared.clone(),
        };
        let mut effective = self.context.as_ref().map_or_else(
            || self.instance.config.context.clone(),
            |c| c.fields.clone(),
        );
        effective.extend(fields.clone());
        let size = match field_size(&effective) {
            Ok(v) => v,
            Err(_) => return self.refuse(Refusal::Invalid),
        };
        // 256 bytes conservatively cover destination and physical-thread strings.
        let Some(limit) = self.shared.config.max_event_bytes.checked_sub(size + 256) else {
            return self.refuse(Refusal::TooLarge);
        };
        let mut message = BoundedText {
            text: String::new(),
            limit,
        };
        if message.write_fmt(args).is_err() {
            return self.refuse(Refusal::TooLarge);
        }
        let physical_thread = self
            .instance
            .config
            .capture_thread
            .then(|| format!("{:?}", thread::current().id()));
        let destination = dest.to_owned();
        let mut s = lock(&self.shared.state);
        if s.health != Health::Open {
            let reason = if s.health == Health::Failed {
                Refusal::Failed
            } else {
                Refusal::Closed
            };
            let result = Shared::refusal(&mut s, self.instance.id, reason);
            drop(s);
            drop(permit);
            return result;
        }
        let stamp = self.shared.clock.now();
        s.sequence += 1;
        let sequence = s.sequence;
        let i = &mut s.instances[self.instance.id];
        i.counters.accepted += 1;
        let epoch = i.epoch;
        s.queue.push_back(Item::Event(Event {
            instance: self.instance.clone(),
            destination,
            level,
            source,
            fields: effective,
            message: message.text,
            thread: physical_thread,
            sequence,
            stamp,
            epoch,
            _permit: permit,
        }));
        self.shared.wake.notify_one();
        EmitResult::Accepted(sequence)
    }
}
