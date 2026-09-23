use crate::{Error, Fields, Level, Logger, Runtime, Source};
use std::sync::Arc;

/// Adapter for the `log` facade. The application owns global registration and
/// must call `Runtime::shutdown` when it is done logging.
///
/// All records use one RLOGGER instance. An exact `log` target matching a
/// declared destination selects that destination; other targets use the
/// instance's module routes. Structured key-value pairs are not copied.
pub struct LogAdapter {
    runtime: Arc<Runtime>,
    logger: Logger,
}

impl LogAdapter {
    /// Reject a logger from a different runtime so `flush` targets the writer
    /// that receives records.
    pub fn new(runtime: Arc<Runtime>, logger: Logger) -> Result<Self, Error> {
        if !Arc::ptr_eq(&runtime.shared, &logger.shared) {
            return Err(Error::Invalid("log adapter logger must share the runtime"));
        }
        Ok(Self { runtime, logger })
    }
}

fn level(level: log::Level) -> Level {
    match level {
        log::Level::Trace => Level::Trace,
        log::Level::Debug => Level::Debug,
        log::Level::Info => Level::Info,
        log::Level::Warn => Level::Warn,
        log::Level::Error => Level::Error,
    }
}

impl log::Log for LogAdapter {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.logger.enabled(level(metadata.level()))
    }

    fn log(&self, record: &log::Record<'_>) {
        let level = level(record.level());
        if !self.logger.enabled(level) {
            self.logger.filtered();
            return;
        }
        let source = Source {
            module: record.module_path_static().unwrap_or(""),
            file: record.file_static().unwrap_or(""),
            line: record.line().unwrap_or(0),
        };
        let destination = self
            .logger
            .instance
            .config
            .destinations
            .iter()
            .find(|name| name.as_str() == record.target())
            .map(String::as_str);
        self.logger
            .emit_args(level, source, destination, &Fields::new(), *record.args());
    }

    fn flush(&self) {
        // The facade has no error channel; Runtime::flush and stats retain the
        // explicit result and failure state for callers that need them.
        let _ = self.runtime.flush();
    }
}
