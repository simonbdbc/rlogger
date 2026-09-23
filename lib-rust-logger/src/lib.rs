//! Ordered local logs, with bounded admission and a single file worker.
//!
//! [`Runtime::flush`] and [`Runtime::shutdown`] block; call them outside async
//! executors (or through `spawn_blocking`). Emission never performs file I/O.
#![doc = include_str!("../README.md")]
mod engine;
#[cfg(feature = "log")]
mod log_adapter;
mod model;
mod output;
pub mod storage;
#[cfg(test)]
mod tests;
#[cfg(feature = "tracing")]
mod tracing_layer;

pub use engine::{Logger, Runtime};
#[cfg(feature = "log")]
pub use log_adapter::LogAdapter;
pub use model::*;
#[cfg(feature = "tracing")]
pub use tracing_layer::LoggerLayer;

/// Filter before evaluating formatting arguments. Returns an [`EmitResult`].
#[macro_export]
macro_rules! log {
    ($logger:expr, $level:expr, $($arg:tt)*) => {{
        let logger = &$logger;
        let level = $level;
        if logger.enabled(level) {
            logger.emit_args(level, $crate::Source { module: module_path!(), file: file!(), line: line!() }, None, &$crate::Fields::new(), format_args!($($arg)*))
        } else { logger.filtered() }
    }};
}
#[macro_export]
macro_rules! info { ($logger:expr, $($arg:tt)*) => { $crate::log!($logger, $crate::Level::Info, $($arg)*) }; }
#[macro_export]
macro_rules! debug { ($logger:expr, $($arg:tt)*) => { $crate::log!($logger, $crate::Level::Debug, $($arg)*) }; }
#[macro_export]
macro_rules! warn { ($logger:expr, $($arg:tt)*) => { $crate::log!($logger, $crate::Level::Warn, $($arg)*) }; }
#[macro_export]
macro_rules! error { ($logger:expr, $($arg:tt)*) => { $crate::log!($logger, $crate::Level::Error, $($arg)*) }; }
#[macro_export]
macro_rules! trace { ($logger:expr, $($arg:tt)*) => { $crate::log!($logger, $crate::Level::Trace, $($arg)*) }; }
