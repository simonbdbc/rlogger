use rlogger::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ordered-logger-demo"));
    let runtime = Runtime::new(Config::new(directory))?;
    println!("{}", runtime.log_root().display());
    let backend = runtime.instance(InstanceConfig::new("backend"))?;
    let mut config = InstanceConfig::new("agents");
    config.capture_thread = true;
    let agents = runtime.instance(config)?;
    std::thread::scope(|scope| {
        for worker in 0..4 {
            let logger = if worker % 2 == 0 {
                backend.clone()
            } else {
                agents.clone()
            };
            scope.spawn(move || {
                let fields = Fields::from([("worker_id".into(), Value::U64(worker))]);
                let operation = logger.with_context(&fields).unwrap();
                for message in ["Début action 🦀", "A", "A", "B", "A", "A", "Fin action"] {
                    info!(operation, "{message}");
                }
            });
        }
    });
    let report = runtime.shutdown()?;
    println!(
        "accepted={} written={} lines={} refused={}",
        report.totals.accepted,
        report.totals.written,
        report.totals.lines,
        report.totals.refused()
    );
    assert_eq!(report.totals.accepted, report.totals.written);
    Ok(())
}
