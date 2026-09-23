use crate::{
    engine::lock,
    model::{BoundedText, field_size},
    *,
};
use std::{
    collections::HashMap,
    fmt::{self, Write},
    sync::{Arc, Mutex},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

const MAX_SPANS: usize = 1024;
const MAX_DEPTH: usize = 32;
const MAX_CAPTURES: usize = 64;
const FIELD_BYTES: usize = 8192;
struct Store {
    spans: HashMap<Id, Fields>,
    active: usize,
}
struct Capture(Arc<Mutex<Store>>);
impl Drop for Capture {
    fn drop(&mut self) {
        lock(&self.0).active -= 1;
    }
}
/// Composable adapter: no global subscriber and no implicit instance creation.
/// Adapter storage: 1,024 spans × 8 KiB, 64 concurrent captures × 8 KiB,
/// plus bounded map overhead. Caller-owned subscriber storage is separate.
pub struct LoggerLayer {
    default: Logger,
    instances: HashMap<String, Logger>,
    store: Arc<Mutex<Store>>,
}
impl LoggerLayer {
    pub fn new(default: Logger) -> Self {
        Self {
            default,
            instances: HashMap::new(),
            store: Arc::new(Mutex::new(Store {
                spans: HashMap::new(),
                active: 0,
            })),
        }
    }
    pub fn with_instance(mut self, logger: Logger) -> Result<Self, Error> {
        if !Arc::ptr_eq(&logger.shared, &self.default.shared) {
            return Err(Error::Invalid("tracing instances must share a runtime"));
        }
        self.instances
            .insert(logger.instance.config.name.clone(), logger);
        Ok(self)
    }
    fn capture(&self) -> Option<Capture> {
        let mut s = lock(&self.store);
        if s.active >= MAX_CAPTURES {
            self.default.refuse(Refusal::Full);
            None
        } else {
            s.active += 1;
            Some(Capture(self.store.clone()))
        }
    }
}
struct Visitor {
    fields: Fields,
    message: Option<String>,
    instance: Option<String>,
    invalid: bool,
}
impl Visitor {
    fn new() -> Self {
        Self {
            fields: Fields::new(),
            message: None,
            instance: None,
            invalid: false,
        }
    }
    fn put(&mut self, field: &Field, value: Value) {
        if self.invalid {
            return;
        }
        if field.name() == "message" {
            if let Value::Text(text) = value {
                self.message = Some(text);
            } else {
                self.invalid = true;
            }
            self.check_size();
            return;
        }
        if field.name() == "log_instance" {
            if let Value::Text(text) = value {
                self.instance = Some(text);
            } else {
                self.invalid = true;
            }
            self.check_size();
            return;
        }
        if field.name().len() > 64 || value.size() > FIELD_BYTES || self.fields.len() >= 64 {
            self.invalid = true;
            return;
        }
        self.fields.insert(field.name().into(), value);
        self.check_size();
    }
    fn check_size(&mut self) {
        let extra = self.message.as_ref().map_or(0, String::len)
            + self.instance.as_ref().map_or(0, String::len);
        if field_size(&self.fields).map_or(true, |n| n + extra > FIELD_BYTES) {
            self.invalid = true;
            self.fields.clear();
            self.message = None;
            self.instance = None;
        }
    }
}
impl Visit for Visitor {
    fn record_str(&mut self, f: &Field, v: &str) {
        if self.invalid {
            return;
        }
        if v.len() > FIELD_BYTES {
            self.invalid = true;
        } else {
            self.put(f, Value::Text(v.into()));
        }
    }
    fn record_i64(&mut self, f: &Field, v: i64) {
        self.put(f, Value::I64(v));
    }
    fn record_u64(&mut self, f: &Field, v: u64) {
        self.put(f, Value::U64(v));
    }
    fn record_bool(&mut self, f: &Field, v: bool) {
        self.put(f, Value::Bool(v));
    }
    fn record_f64(&mut self, f: &Field, v: f64) {
        self.put(f, Value::F64(v));
    }
    fn record_debug(&mut self, f: &Field, v: &dyn fmt::Debug) {
        if self.invalid {
            return;
        }
        let mut text = BoundedText {
            text: String::new(),
            limit: FIELD_BYTES,
        };
        if write!(&mut text, "{v:?}").is_err() {
            self.invalid = true;
        } else {
            self.put(f, Value::Text(text.text));
        }
    }
}
impl<S> Layer<S> for LoggerLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
        let Some(_capture) = self.capture() else {
            return;
        };
        let mut visitor = Visitor::new();
        attrs.record(&mut visitor);
        if visitor.invalid {
            self.default.refuse(Refusal::Invalid);
            return;
        }
        let mut store = lock(&self.store);
        if store.spans.len() >= MAX_SPANS {
            self.default.refuse(Refusal::Full);
            return;
        }
        store.spans.insert(id.clone(), visitor.fields);
    }
    fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        let Some(_capture) = self.capture() else {
            return;
        };
        let mut v = Visitor::new();
        values.record(&mut v);
        let mut store = lock(&self.store);
        if v.invalid {
            store.spans.remove(id);
            self.default.refuse(Refusal::Invalid);
            return;
        }
        if let Some(fields) = store.spans.get_mut(id) {
            fields.extend(v.fields);
            if field_size(fields).map_or(true, |n| n > FIELD_BYTES) {
                store.spans.remove(id);
                self.default.refuse(Refusal::TooLarge);
            }
        }
    }
    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        lock(&self.store).spans.remove(&id);
    }
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let Some(_capture) = self.capture() else {
            return;
        };
        let mut v = Visitor::new();
        event.record(&mut v);
        if v.invalid {
            self.default.refuse(Refusal::Invalid);
            return;
        }
        let logger = match v.instance.as_ref() {
            None => &self.default,
            Some(name) if name == &self.default.instance.config.name => &self.default,
            Some(name) => match self.instances.get(name) {
                Some(l) => l,
                None => {
                    self.default.refuse(Refusal::UnknownDestination);
                    return;
                }
            },
        };
        let mut fields = Fields::new();
        if let Some(scope) = ctx.event_scope(event) {
            let spans = scope.take(MAX_DEPTH + 1).collect::<Vec<_>>();
            if spans.len() > MAX_DEPTH {
                logger.refuse(Refusal::TooLarge);
                return;
            }
            let store = lock(&self.store);
            for span in spans.into_iter().rev() {
                let Some(inherited) = store.spans.get(&span.id()) else {
                    logger.refuse(Refusal::Invalid);
                    return;
                };
                for (key, value) in inherited {
                    fields.insert(key.clone(), value.clone());
                }
                if field_size(&fields).map_or(true, |n| n > FIELD_BYTES) {
                    logger.refuse(Refusal::TooLarge);
                    return;
                }
            }
        }
        fields.extend(v.fields);
        let m = event.metadata();
        let level = match *m.level() {
            tracing::Level::TRACE => Level::Trace,
            tracing::Level::DEBUG => Level::Debug,
            tracing::Level::INFO => Level::Info,
            tracing::Level::WARN => Level::Warn,
            tracing::Level::ERROR => Level::Error,
        };
        let source = Source {
            module: m.module_path().unwrap_or(m.target()),
            file: m.file().unwrap_or(""),
            line: m.line().unwrap_or(0),
        };
        let target = logger
            .instance
            .config
            .destinations
            .iter()
            .find(|d| d.as_str() == m.target())
            .map(String::as_str);
        logger.emit(
            level,
            source,
            target,
            &fields,
            v.message.as_deref().unwrap_or(m.name()),
        );
    }
}
