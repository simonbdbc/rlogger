use crate::{
    engine::lock,
    output::{Clock, Output, Stamp},
    *,
};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime},
};

type ClockState = (
    u64,
    DateTime<FixedOffset>,
    BTreeMap<SystemTime, FixedOffset>,
);
#[derive(Clone)]
struct Time(Arc<Mutex<ClockState>>);
impl Time {
    fn new() -> Self {
        Self(Arc::new(Mutex::new((
            0,
            DateTime::parse_from_rfc3339("2026-09-08T12:00:00+02:00").unwrap(),
            BTreeMap::new(),
        ))))
    }
    fn set(&self, mono: u64, civil: &str) {
        let mut state = lock(&self.0);
        let civil = DateTime::parse_from_rfc3339(civil).unwrap();
        state.0 = mono;
        state.1 = civil;
        state.2.insert(civil.into(), *civil.offset());
    }
}
impl Clock for Time {
    fn now(&self) -> Stamp {
        let t = lock(&self.0);
        Stamp {
            mono: Duration::from_millis(t.0),
            wall: t.1.into(),
        }
    }
    fn local(&self, wall: SystemTime) -> DateTime<FixedOffset> {
        let state = lock(&self.0);
        DateTime::<chrono::Utc>::from(wall)
            .with_timezone(state.2.get(&wall).unwrap_or(state.1.offset()))
    }
}
type Lines = Arc<Mutex<BTreeMap<String, String>>>;
struct Memory {
    lines: Lines,
    fail_write: bool,
    fail_flush: bool,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}
impl Output for Memory {
    fn write(&mut self, path: &str, line: &str) -> io::Result<()> {
        if let Some((started, release)) = self.gate.take() {
            started.send(()).unwrap();
            release.recv().unwrap();
        }
        if self.fail_write {
            return Err(io::Error::other("injected write"));
        }
        lock(&self.lines)
            .entry(path.into())
            .or_default()
            .push_str(line);
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::other("injected flush"))
        } else {
            Ok(())
        }
    }
}
fn setup(mut config: Config) -> (Runtime, Logger, Lines, Time) {
    config.group_duration = Duration::from_millis(200);
    let lines = Lines::default();
    let clock = Time::new();
    let rt = Runtime::start(
        config,
        Arc::new(clock.clone()),
        Box::new(Memory {
            lines: lines.clone(),
            fail_write: false,
            fail_flush: false,
            gate: None,
        }),
        "memory".into(),
    )
    .unwrap();
    let logger = rt.instance(InstanceConfig::new("test")).unwrap();
    (rt, logger, lines, clock)
}
fn emit(logger: &Logger, message: &str) -> EmitResult {
    logger.emit(
        Level::Info,
        Source::default(),
        None,
        &Fields::new(),
        message,
    )
}
fn all(lines: &Lines) -> String {
    lock(lines).values().cloned().collect()
}
#[test]
fn consecutive_groups_and_escaped_fields() {
    let (rt, l, lines, _) = setup(Config::new("unused"));
    for m in ["A", "A", "B", "A", "A"] {
        assert!(matches!(emit(&l, m), EmitResult::Accepted(_)));
    }
    let report = rt.shutdown().unwrap();
    assert_eq!(report.totals.written, 5);
    assert_eq!(report.totals.lines, 3);
    assert_eq!(report.reserved_bytes, 0);
    let s = all(&lines);
    assert_eq!(s.lines().filter(|l| l.ends_with("A x2")).count(), 2);
    assert!(s.contains("seq_first=4 seq_last=5 count=2"));
    assert_eq!(s.matches("LATENCY").count(), 3);
}
#[test]
fn concurrent_acceptance_reconciles_sequences() {
    let (rt, l, lines, _) = setup(Config::new("unused"));
    let accepted = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let l = l.clone();
            let accepted = accepted.clone();
            scope.spawn(move || {
                for index in 0..100 {
                    let msg = format!("{worker}:{index}");
                    if let EmitResult::Accepted(seq) = emit(&l, &msg) {
                        lock(&accepted).push((seq, msg));
                    }
                }
            });
        }
    });
    let report = rt.shutdown().unwrap();
    assert_eq!(report.totals.written, 800);
    assert_eq!(report.pending, 0);
    let text = all(&lines);
    let output = text.lines().collect::<Vec<_>>();
    let mut expected = lock(&accepted).clone();
    expected.sort();
    for (line, (seq, msg)) in output.iter().zip(expected) {
        assert!(line.contains(&format!("[seq={seq} ")));
        assert!(line.ends_with(&msg));
    }
    assert_eq!(output.len(), 800);
}
#[test]
fn producer_suspended_in_formatting_has_no_sequence_and_shutdown_does_not_wait() {
    struct Slow(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>);
    impl std::fmt::Display for Slow {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.send(()).unwrap();
            lock(&self.1).recv().unwrap();
            f.write_str("late")
        }
    }
    let (rt, l, lines, _) = setup(Config::new("unused"));
    let (started, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let late = l.clone();
    let thread = std::thread::spawn(move || info!(late, "{}", Slow(started, Mutex::new(resume))));
    wait.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(emit(&l, "first"), EmitResult::Accepted(1));
    let report = rt.shutdown().unwrap();
    assert_eq!(report.totals.written, 1);
    release.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), EmitResult::Refused(Refusal::Closed));
    assert_eq!(l.stats().reserved_bytes, 0);
    assert!(all(&lines).ends_with("first\n"));
}
#[test]
fn panic_during_formatting_returns_reservation() {
    struct Bad;
    impl std::fmt::Display for Bad {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("user formatter");
        }
    }
    let (rt, l, _, _) = setup(Config::new("unused"));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| info!(l, "{}", Bad))).is_err()
    );
    assert_eq!(rt.stats().reserved_bytes, 0);
    assert_eq!(emit(&l, "ok"), EmitResult::Accepted(1));
    rt.shutdown().unwrap();
}
#[test]
fn overload_control_survives_full_queue_and_breaks_continuity() {
    let mut cfg = Config::new("unused");
    cfg.max_events = 1;
    let (rt, l, lines, _) = setup(cfg);
    assert_eq!(emit(&l, "A"), EmitResult::Accepted(1));
    assert_eq!(emit(&l, "A"), EmitResult::Refused(Refusal::Full));
    rt.flush().unwrap();
    assert!(matches!(emit(&l, "A"), EmitResult::Accepted(_)));
    let r = rt.shutdown().unwrap();
    assert_eq!(r.totals.full, 1);
    assert_eq!(r.totals.overload_reported, 1);
    assert_eq!(r.totals.written, 2);
    assert!(!all(&lines).contains("x2"));
}
#[test]
fn context_is_immutable_bounded_and_overridden_by_event() {
    let (rt, l, lines, _) = setup(Config::new("unused"));
    let a = l
        .with_context(&Fields::from([("call_id".into(), "a".into())]))
        .unwrap();
    let b = l
        .with_context(&Fields::from([("call_id".into(), "b".into())]))
        .unwrap();
    emit(&a, "A");
    emit(&b, "A");
    let fields = Fields::from([
        ("call_id".into(), "override".into()),
        ("note".into(), "a\nb\r\t\u{1b}".into()),
    ]);
    a.emit(
        Level::Info,
        Source::default(),
        None,
        &fields,
        "Unicode 🦀\nnext",
    );
    rt.shutdown().unwrap();
    let text = all(&lines);
    assert_eq!(text.lines().count(), 3);
    assert!(text.contains("call_id=\"a\""));
    assert!(text.contains("call_id=\"b\""));
    assert!(text.contains("call_id=\"override\""));
    assert!(text.contains("Unicode 🦀\\nnext"));
    drop(a);
    drop(b);
    assert_eq!(l.stats().context_bytes, 0);
}
#[test]
fn oversized_invalid_filtered_and_unknown_are_distinct() {
    let (rt, l, _, _) = setup(Config::new("unused"));
    let mut called = false;
    assert_eq!(
        debug!(l, "{}", {
            called = true;
            "never"
        }),
        EmitResult::Filtered
    );
    assert!(!called);
    assert_eq!(
        emit(&l, &"x".repeat(8193)),
        EmitResult::Refused(Refusal::TooLarge)
    );
    assert_eq!(
        l.emit(
            Level::Info,
            Source::default(),
            Some("../escape"),
            &Fields::new(),
            "x"
        ),
        EmitResult::Refused(Refusal::UnknownDestination)
    );
    assert_eq!(
        l.emit(
            Level::Info,
            Source::default(),
            None,
            &Fields::from([("seq".into(), 1u64.into())]),
            "x"
        ),
        EmitResult::Refused(Refusal::Invalid)
    );
    assert_eq!(
        l.emit(
            Level::Info,
            Source::default(),
            None,
            &Fields::from([("v".into(), f64::NAN.into())]),
            "x"
        ),
        EmitResult::Refused(Refusal::Invalid)
    );
    assert_eq!(rt.stats().reserved_bytes, 0);
    rt.shutdown().unwrap();
}
#[test]
fn configuration_and_registry_reject_invalid_limits_and_names() {
    let mut c = Config::new("unused");
    c.max_events = 0;
    assert!(c.validate().is_err());
    let (rt, _, _, _) = setup(Config::new("unused"));
    for name in ["Test", "../escape", "test", "con", ""] {
        assert!(rt.instance(InstanceConfig::new(name)).is_err());
    }
    let mut c = InstanceConfig::new("routes");
    c.destinations = vec!["a".into(), "a".into()];
    assert!(rt.instance(c).is_err());
    rt.shutdown().unwrap();
    assert!(rt.instance(InstanceConfig::new("closed")).is_err());
}
#[test]
fn routing_uses_segments_and_longest_prefix() {
    let (rt, _, lines, _) = setup(Config::new("unused"));
    let mut c = InstanceConfig::new("router");
    c.destinations = vec!["default".into(), "module".into(), "nested".into()];
    c.routes = vec![
        Route {
            module_prefix: "app::http".into(),
            destination: "module".into(),
        },
        Route {
            module_prefix: "app::http::api".into(),
            destination: "nested".into(),
        },
    ];
    let l = rt.instance(c).unwrap();
    for module in ["app::http", "app::https", "app::http::api::get"] {
        l.emit(
            Level::Info,
            Source {
                module,
                ..Source::default()
            },
            None,
            &Fields::new(),
            module,
        );
    }
    rt.shutdown().unwrap();
    let output = lock(&lines);
    assert!(output["2026-09-08/router/default-12-h1788861600"].contains("app::https"));
    assert!(output["2026-09-08/router/module-12-h1788861600"].ends_with("app::http\n"));
    assert!(output["2026-09-08/router/nested-12-h1788861600"].ends_with("app::http::api::get\n"));
}
#[test]
fn disk_errors_fail_stop_and_keep_unconfirmed_counts() {
    for (write, flush) in [(true, false), (false, true)] {
        let rt = Runtime::start(
            Config::new("unused"),
            Arc::new(Time::new()),
            Box::new(Memory {
                lines: Lines::default(),
                fail_write: write,
                fail_flush: flush,
                gate: None,
            }),
            "memory".into(),
        )
        .unwrap();
        let l = rt.instance(InstanceConfig::new("a")).unwrap();
        emit(&l, "A");
        let e = rt.shutdown().unwrap_err();
        assert_eq!(e.report.pending, 1);
        assert_eq!(e.report.totals.written, 0);
        assert_eq!(e.report.reserved_bytes, 0);
        assert_eq!(emit(&l, "B"), EmitResult::Refused(Refusal::Failed));
        assert!(rt.shutdown().is_err());
    }
}
#[test]
fn blocked_output_does_not_hold_admission_lock() {
    let (start, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut c = Config::new("unused");
    c.max_events = 4;
    let rt = Arc::new(
        Runtime::start(
            c,
            Arc::new(Time::new()),
            Box::new(Memory {
                lines: Lines::default(),
                fail_write: false,
                fail_flush: false,
                gate: Some((start, wait)),
            }),
            "memory".into(),
        )
        .unwrap(),
    );
    let l = rt.instance(InstanceConfig::new("a")).unwrap();
    emit(&l, "A");
    emit(&l, "B");
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let (done, received) = mpsc::channel();
    let producer = l.clone();
    std::thread::spawn(move || {
        for _ in 0..10000 {
            emit(&producer, "C");
        }
        done.send(()).unwrap();
    });
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(rt.stats().totals.full > 0);
    assert!(rt.stats().reserved_events <= 4);
    assert!(rt.stats().reserved_bytes <= 4 * 8192);
    let (stopped, stopping) = mpsc::channel();
    let closer = rt.clone();
    let shutdown = std::thread::spawn(move || stopped.send(closer.shutdown()).unwrap());
    assert!(stopping.try_recv().is_err());
    release.send(()).unwrap();
    let r = stopping
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    shutdown.join().unwrap();
    assert_eq!(r.totals.written, r.totals.accepted);
    assert_eq!(r.totals.full, r.totals.overload_reported);
}
#[test]
fn actual_directory_creation_failure_is_reported_without_replaying() {
    let temp = tempfile::tempdir().unwrap();
    let not_directory = temp.path().join("file");
    std::fs::write(&not_directory, "keep").unwrap();
    assert!(Runtime::new(Config::new(&not_directory)).is_err());
    let rt = Runtime::new(Config::new(temp.path().join("logs"))).unwrap();
    let logger = rt.instance(InstanceConfig::new("blocked")).unwrap();
    let blocked = rt
        .log_root()
        .join(chrono::Local::now().format("%Y-%m-%d").to_string());
    std::fs::write(&blocked, "keep").unwrap();
    assert!(matches!(
        emit(&logger, "accepted before disk failure"),
        EmitResult::Accepted(_)
    ));
    let error = rt.shutdown().unwrap_err();
    assert_eq!(error.report.pending, 1);
    assert_eq!(error.report.totals.written, 0);
    assert_eq!(std::fs::read_to_string(blocked).unwrap(), "keep");
    assert_eq!(
        emit(&logger, "after failure"),
        EmitResult::Refused(Refusal::Failed)
    );
}
#[test]
fn monotone_latency_ignores_civil_rollback() {
    let (rt, l, lines, time) = setup(Config::new("unused"));
    emit(&l, "A");
    time.set(175, "2026-09-08T11:00:00+02:00");
    rt.flush().unwrap();
    assert!(all(&lines).contains("LATENCY — 175ms"));
    rt.shutdown().unwrap();
}
#[test]
fn hourly_sealing_without_traffic_and_late_segments_are_immutable() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::new(temp.path());
    let output = crate::output::FileOutput::new(&config).unwrap();
    let root = output.log_root.clone();
    let run = output.run_id.clone();
    let time = Time::new();
    let runtime = Runtime::start(
        config,
        Arc::new(time.clone()),
        Box::new(output),
        root.clone(),
    )
    .unwrap();
    let logger = runtime.instance(InstanceConfig::new("test")).unwrap();
    emit(&logger, "before");
    runtime.flush().unwrap();
    let dir = root.join("2026-09-08/test");
    let active = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(active.to_str().unwrap().ends_with(".active.log"));
    let lease =
        crate::storage::open_file(&crate::storage::lease_path(&root, &run), true, false).unwrap();
    assert!(lease.try_lock().is_err());
    time.set(3600000, "2026-09-08T13:00:00+02:00");
    runtime.shared.wake.notify_one();
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while active.exists() && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!active.exists());
    let closed = active.with_file_name(
        active
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".active.log", ".log"),
    );
    let first = std::fs::read(&closed).unwrap();
    time.set(3600001, "2026-09-08T12:30:00+02:00");
    emit(&logger, "late");
    let report = runtime.shutdown().unwrap();
    assert_eq!(report.totals.written, 2);
    assert_eq!(std::fs::read(closed).unwrap(), first);
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 2);
    lease.try_lock().unwrap();
}

#[test]
fn failed_publication_never_replaces_existing_file_or_replays_buffers() {
    let temp = tempfile::tempdir().unwrap();
    let mut out = crate::output::FileOutput::new(&Config::new(temp.path())).unwrap();
    out.write("2026-09-08/test/app-12-h1788861600", "one\n")
        .unwrap();
    out.flush().unwrap();
    let active = std::fs::read_dir(out.log_root.join("2026-09-08/test"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let target = active.with_file_name(
        active
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".active.log", ".log"),
    );
    std::fs::write(&target, "existing").unwrap();
    assert!(out.close_all().is_err());
    drop(out);
    assert_eq!(std::fs::read_to_string(target).unwrap(), "existing");
    assert_eq!(std::fs::read_to_string(active).unwrap(), "one\n");
}
#[test]
fn concurrent_writers_and_empty_day_cleanup_share_the_same_gate() {
    use crate::{output::FileOutput, storage};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("rlogger");
    storage::initialize(&root).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let parent = temp.path();
            scope.spawn(move || {
                let mut writer = FileOutput::new(&Config::new(parent)).unwrap();
                for _ in 0..30 {
                    writer
                        .write("2000-01-01/test/app-00-h946684800", "accepted\n")
                        .unwrap();
                    writer.close_all().unwrap();
                }
            });
        }
        let root = &root;
        scope.spawn(move || {
            for _ in 0..120 {
                let _g = storage::gate(root).unwrap();
                for path in ["2000-01-01/test", "2000-01-01"] {
                    match storage::remove_beneath(root, std::path::Path::new(path), true, None) {
                        Ok(()) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                            ) => {}
                        Err(e) => panic!("unexpected cleanup failure: {e}"),
                    }
                }
                std::thread::yield_now();
            }
        });
    });
    let files = std::fs::read_dir(root.join("2000-01-01/test"))
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 120);
    let mut runs = std::collections::HashSet::new();
    for file in files {
        let file = file.unwrap();
        let log = storage::LogName::parse(file.file_name().to_str().unwrap()).unwrap();
        assert_eq!(log.state, storage::FileState::Closed);
        runs.insert(log.run_id);
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "accepted\n");
    }
    assert_eq!(runs.len(), 4);
}
#[test]
fn actual_files_unique_runs_lru_reopen_and_capture_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::new(temp.path());
    config.max_writers = 1;
    let rt = Runtime::new(config.clone()).unwrap();
    let second = Runtime::new(config).unwrap();
    assert_eq!(rt.log_root(), second.log_root());
    assert_ne!(rt.run_id(), second.run_id());
    let mut instance = InstanceConfig::new("a");
    instance.destinations = vec!["one".into(), "two".into()];
    let l = rt.instance(instance).unwrap();
    for d in ["one", "two", "one"] {
        l.emit(Level::Info, Source::default(), Some(d), &Fields::new(), d);
        rt.flush().unwrap();
    }
    let r = rt.shutdown().unwrap();
    assert_eq!(r.totals.written, 3);
    let day = std::fs::read_dir(rt.log_root())
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .unwrap()
        .path()
        .join("a");
    let mut lines = 0;
    for entry in std::fs::read_dir(day).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(crate::storage::LogName::parse(name).is_some());
        assert!(!name.ends_with(".active.log"));
        lines += std::fs::read_to_string(p).unwrap().lines().count();
    }
    assert_eq!(lines, 3);
    second.shutdown().unwrap();
}
#[test]
fn controlled_dst_paths_distinguish_repeated_hour_and_skip_missing_hour() {
    let i = crate::engine::Instance {
        id: 0,
        config: InstanceConfig {
            ..InstanceConfig::new("a")
        },
    };
    let parse = |s| DateTime::parse_from_rfc3339(s).unwrap();
    assert_ne!(
        crate::output::path_for(&i, "app", &parse("2026-10-25T02:30:00+02:00")),
        crate::output::path_for(&i, "app", &parse("2026-10-25T02:30:00+01:00"))
    );
    assert!(
        crate::output::path_for(&i, "app", &parse("2026-03-29T03:00:00+02:00"))
            .contains("app-03-h")
    );
    assert!(
        crate::output::path_for(&i, "app", &parse("2026-09-08T23:59:59+02:00"))
            .contains("2026-09-08/")
    );
}

#[test]
fn noncontiguous_global_sequences_and_refusal_boundaries() {
    let (rt, _, lines, _) = setup(Config::new("unused"));
    let mut c = InstanceConfig::new("projection");
    c.destinations = vec!["a".into(), "b".into()];
    let logger = rt.instance(c).unwrap();
    logger.emit(
        Level::Info,
        Source::default(),
        Some("a"),
        &Fields::new(),
        "A",
    );
    logger.emit(
        Level::Info,
        Source::default(),
        Some("b"),
        &Fields::new(),
        "B",
    );
    logger.emit(
        Level::Info,
        Source::default(),
        Some("a"),
        &Fields::new(),
        "A",
    );
    logger.emit(
        Level::Info,
        Source::default(),
        Some("unknown"),
        &Fields::new(),
        "refused",
    );
    logger.emit(
        Level::Info,
        Source::default(),
        Some("a"),
        &Fields::new(),
        "A",
    );
    let report = rt.shutdown().unwrap();
    assert_eq!(report.totals.written, 4);
    let s = lock(&lines);
    let a = &s["2026-09-08/projection/a-12-h1788861600"];
    assert!(a.contains("seq_first=1 seq_last=3 count=2"));
    assert_eq!(a.lines().count(), 2);
}

#[test]
fn delayed_midnight_and_repeated_hour_keep_capture_paths_and_split_groups() {
    let (rt, _, lines, time) = setup(Config::new("unused"));
    let c = InstanceConfig::new("rotation");
    let logger = rt.instance(c).unwrap();
    time.set(0, "2026-09-08T23:59:59+02:00");
    emit(&logger, "midnight");
    time.set(1, "2026-09-09T00:00:00+02:00");
    emit(&logger, "midnight");
    time.set(2, "2026-10-25T02:30:00+02:00");
    emit(&logger, "repeated");
    time.set(3, "2026-10-25T02:30:00+01:00");
    emit(&logger, "repeated");
    rt.shutdown().unwrap();
    let output = lock(&lines);
    assert!(
        output
            .keys()
            .any(|s| s.starts_with("2026-09-08/rotation/app-23-h"))
    );
    assert!(
        output
            .keys()
            .any(|s| s.starts_with("2026-09-09/rotation/app-00-h"))
    );
    let repeated = output
        .iter()
        .filter(|(p, _)| p.starts_with("2026-10-25/rotation/app-02-h"))
        .map(|(_, s)| s.as_str())
        .collect::<String>();
    assert_eq!(repeated.lines().count(), 2);
    assert!(repeated.contains("+02:00"));
    assert!(repeated.contains("+01:00"));
    assert!(!repeated.contains("x2"));
}

#[test]
fn isolated_group_deadline_flushes_without_another_event() {
    struct Signaling(mpsc::Sender<String>);
    impl Output for Signaling {
        fn write(&mut self, _: &str, line: &str) -> io::Result<()> {
            self.0.send(line.into()).unwrap();
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let time = Time::new();
    let (tx, rx) = mpsc::channel();
    let rt = Runtime::start(
        Config::new("unused"),
        Arc::new(time.clone()),
        Box::new(Signaling(tx)),
        "memory".into(),
    )
    .unwrap();
    let logger = rt.instance(InstanceConfig::new("deadline")).unwrap();
    emit(&logger, "isolated");
    time.set(201, "2026-09-08T12:00:00.201+02:00");
    rt.shared.wake.notify_one();
    let line = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(line.contains("LATENCY — 201ms"));
    assert!(line.ends_with("isolated\n"));
    assert_eq!(rt.shutdown().unwrap().totals.written, 1);
}

#[test]
fn context_budget_frees_on_last_clone_and_drop_drains() {
    let mut c = Config::new("unused");
    c.max_context_bytes = 8192;
    let (rt, logger, lines, _) = setup(c);
    let context = logger
        .with_context(&Fields::from([("call_id".into(), "one".into())]))
        .unwrap();
    let clone = context.clone();
    assert!(logger.with_context(&Fields::new()).is_err());
    drop(context);
    assert_eq!(logger.stats().context_bytes, 8192);
    drop(clone);
    assert_eq!(logger.stats().context_bytes, 0);
    emit(&logger, "drop drains");
    drop(rt);
    assert!(all(&lines).ends_with("drop drains\n"));
    assert_eq!(
        emit(&logger, "closed"),
        EmitResult::Refused(Refusal::Closed)
    );
}

#[test]
fn shared_contract_fixture_matches_writer() {
    let (rt, logger, lines, _) = setup(Config::new("unused"));
    let source = Source {
        module: "fixture",
        file: "fixture.rs",
        line: 1,
    };
    let fields = Fields::from([("call_id".into(), "c1".into())]);
    for message in ["A", "A", "B", "A", "A"] {
        logger.emit(Level::Info, source, None, &fields, message);
    }
    logger.emit(
        Level::Error,
        source,
        None,
        &fields,
        "Erreur 🦀\nligne\t\u{1b}",
    );
    let report = rt.shutdown().unwrap();
    assert_eq!(report.totals.written, 6);
    assert_eq!(all(&lines), include_str!("../fixtures/v1/rust.log"));
}

#[cfg(feature = "tracing")]
#[test]
fn tracing_span_limits_release_and_async_tasks_do_not_leak_context() {
    use tracing::Instrument;
    use tracing_subscriber::prelude::*;
    let (rt, logger, lines, _) = setup(Config::new("unused"));
    let subscriber = tracing_subscriber::registry().with(LoggerLayer::new(logger.clone()));
    tracing::subscriber::with_default(subscriber, || {
        let spans = (0..1024)
            .map(|id| tracing::info_span!("slot", slot = id))
            .collect::<Vec<_>>();
        let overflow = tracing::info_span!("overflow");
        assert_eq!(logger.counters().full, 1);
        drop(overflow);
        drop(spans);
        let executor = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        executor.block_on(async {
            let a = async {
                tracing::info!("a-start");
                tokio::task::yield_now().await;
                tracing::info!("a-end");
            }
            .instrument(tracing::info_span!("operation", call_id = "a"));
            let b = async {
                tracing::info!("b-start");
                tokio::task::yield_now().await;
                tracing::info!("b-end");
            }
            .instrument(tracing::info_span!("operation", call_id = "b"));
            tokio::join!(a, b);
        });
        let mut chain = Vec::new();
        for _ in 0..33 {
            chain.push(if let Some(parent) = chain.last() {
                tracing::info_span!(parent:parent,"deep")
            } else {
                tracing::info_span!("root")
            });
        }
        tracing::info!(parent:chain.last().unwrap(),"too deep");
        assert_eq!(logger.counters().too_large, 1);
    });
    assert_eq!(rt.shutdown().unwrap().totals.written, 4);
    for line in all(&lines).lines().filter(|l| !l.contains("OVERLOAD")) {
        if line.ends_with("a-start") || line.ends_with("a-end") {
            assert!(line.contains("call_id=\"a\""));
        } else {
            assert!(line.contains("call_id=\"b\""));
        }
    }
}
#[cfg(feature = "tracing")]
#[test]
fn tracing_spans_updates_explicit_parent_and_native_share_admission() {
    use tracing_subscriber::prelude::*;
    let (rt, l, lines, _) = setup(Config::new("unused"));
    let subscriber = tracing_subscriber::registry().with(LoggerLayer::new(l.clone()));
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("operation", call_id = "before");
        span.record("call_id", "after");
        emit(&l, "native");
        tracing::info!(parent:&span,event_id=7u64,"traced");
        tracing::info!(log_instance = "missing", "refused");
    });
    let r = rt.shutdown().unwrap();
    assert_eq!(r.totals.written, 2);
    assert_eq!(r.totals.unknown_destination, 1);
    assert!(all(&lines).contains("call_id=\"after\""));
    assert!(all(&lines).contains("[seq=2 "));
}
