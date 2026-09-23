use std::{collections::BTreeMap, fmt, path::PathBuf, time::Duration};

/// Ascending severity. Events below the instance threshold are filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}
impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        })
    }
}
/// Field values are typed. Floats compare by bits; non-finite values are rejected.
#[derive(Clone, Debug)]
pub enum Value {
    Text(String),
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::U64(a), Self::U64(b)) => a == b,
            (Self::F64(a), Self::F64(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}
impl Eq for Value {}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Text(v.to_owned())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}
impl Value {
    pub(crate) fn size(&self) -> usize {
        match self {
            Self::Text(v) => v.len(),
            _ => 8,
        }
    }
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Text(v) => format!("\"{}\"", escape(v)),
            Self::Bool(v) => v.to_string(),
            Self::I64(v) => v.to_string(),
            Self::U64(v) => v.to_string(),
            Self::F64(v) => format!("{v:?}"),
        }
    }
}
/// Sorted map; inserting the same key replaces its previous value.
pub type Fields = BTreeMap<String, Value>;
pub(crate) fn field_size(fields: &Fields) -> Result<usize, Error> {
    if fields.len() > 64 {
        return Err(Error::Invalid("at most 64 fields"));
    }
    let mut size = 0usize;
    for (k, v) in fields {
        if k.is_empty()
            || k.len() > 64
            || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || matches!(
                k.as_str(),
                "seq"
                    | "seq_first"
                    | "seq_last"
                    | "latency"
                    | "instance"
                    | "run"
                    | "count"
                    | "source"
                    | "thread"
                    | "timestamp"
            )
        {
            return Err(Error::Invalid("invalid or reserved field key"));
        }
        if matches!(v,Value::F64(x) if !x.is_finite()) {
            return Err(Error::Invalid("non-finite field"));
        }
        size = size
            .checked_add(k.len() + v.size() + 96)
            .ok_or(Error::Invalid("field size overflow"))?;
    }
    Ok(size)
}
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !matches!(name, "con" | "prn" | "aux" | "nul" | "com1" | "lpt1")
}
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Source {
    pub module: &'static str,
    pub file: &'static str,
    pub line: u32,
}
#[derive(Clone, Debug)]
pub struct Route {
    pub module_prefix: String,
    pub destination: String,
}
#[derive(Clone, Debug)]
pub struct InstanceConfig {
    pub name: String,
    pub level: Level,
    pub context: Fields,
    pub destinations: Vec<String>,
    pub routes: Vec<Route>,
    pub capture_thread: bool,
}
impl InstanceConfig {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: Level::Info,
            context: Fields::new(),
            destinations: vec!["app".into()],
            routes: Vec::new(),
            capture_thread: false,
        }
    }
}
/// Resource defaults are provisional tuning values, not latency guarantees.
#[derive(Clone, Debug)]
pub struct Config {
    pub directory: PathBuf,
    pub max_events: usize,
    pub max_bytes: usize,
    pub max_event_bytes: usize,
    pub max_instances: usize,
    pub max_destinations: usize,
    pub max_writers: usize,
    pub writer_buffer_bytes: usize,
    pub max_context_bytes: usize,
    pub group_duration: Duration,
    pub flush_interval: Duration,
    pub overload_interval: Duration,
    pub context: Fields,
}
impl Config {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            max_events: 8192,
            max_bytes: 32 * 1024 * 1024,
            max_event_bytes: 8192,
            max_instances: 32,
            max_destinations: 128,
            max_writers: 32,
            writer_buffer_bytes: 65536,
            max_context_bytes: 1024 * 1024,
            group_duration: Duration::from_millis(200),
            flush_interval: Duration::from_millis(100),
            overload_interval: Duration::from_secs(1),
            context: Fields::new(),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.directory.as_os_str().is_empty()
            || self.max_events == 0
            || self.max_event_bytes < 256
            || self.max_bytes < self.max_event_bytes
            || self.max_instances == 0
            || self.max_destinations == 0
            || self.max_writers == 0
            || self.writer_buffer_bytes == 0
            || self.max_context_bytes < self.max_event_bytes
            || self.group_duration.is_zero()
            || self.flush_interval.is_zero()
            || self.overload_interval.is_zero()
            || self
                .group_duration
                .checked_add(self.flush_interval)
                .is_none_or(|d| d > Duration::from_secs(1))
        {
            return Err(Error::Invalid("invalid resource or duration limits"));
        }
        if field_size(&self.context)? > self.max_event_bytes {
            return Err(Error::Invalid("application context too large"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid(&'static str),
    Limit(&'static str),
    Closed,
    Failed(String),
    Io(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Full,
    TooLarge,
    Closed,
    Failed,
    Invalid,
    UnknownDestination,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitResult {
    Accepted(u64),
    Filtered,
    Refused(Refusal),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Health {
    #[default]
    Open,
    Closing,
    Closed,
    Failed,
}
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub accepted: u64,
    pub filtered: u64,
    pub full: u64,
    pub too_large: u64,
    pub closed: u64,
    pub failed: u64,
    pub invalid: u64,
    pub unknown_destination: u64,
    pub written: u64,
    pub lines: u64,
    pub overload_reported: u64,
}
impl Counters {
    pub fn refused(&self) -> u64 {
        self.full
            + self.too_large
            + self.closed
            + self.failed
            + self.invalid
            + self.unknown_destination
    }
}
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub health: Health,
    pub totals: Counters,
    pub instances: BTreeMap<String, Counters>,
    pub pending: u64,
    pub reserved_events: usize,
    pub reserved_bytes: usize,
    pub context_bytes: usize,
    pub error: Option<String>,
}
/// A failed shutdown retains its reconciliation report.
#[derive(Clone, Debug)]
pub struct ShutdownError {
    pub report: Box<Report>,
}
impl fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "logger failed: {:?}", self.report.error)
    }
}
impl std::error::Error for ShutdownError {}

pub(crate) struct BoundedText {
    pub text: String,
    pub limit: usize,
}
impl fmt::Write for BoundedText {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.len() > self.limit.saturating_sub(self.text.len()) {
            return Err(fmt::Error);
        }
        self.text.push_str(s);
        Ok(())
    }
}
