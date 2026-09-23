use rlogger::{Config, InstanceConfig, LogAdapter, Runtime};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("rlogger-log-demo"));
    let runtime = Arc::new(Runtime::new(Config::new(directory))?);
    let logger = runtime.instance(InstanceConfig::new("backend"))?;

    log::set_boxed_logger(Box::new(LogAdapter::new(runtime.clone(), logger)?))?;
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("Log facade event");
    log::logger().flush();

    let report = runtime.shutdown()?;
    assert_eq!(report.totals.accepted, report.totals.written);
    println!("{} accepted and written", report.totals.written);
    Ok(())
}
