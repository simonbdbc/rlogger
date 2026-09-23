# RLOGGER

RLOGGER 0.2.0 is a bounded, ordered local file logger for Rust 1.95+ (edition
2024), licensed under MIT. A dedicated worker writes events for multiple named
instances. The library starts no server, async runtime, or global subscriber.

```toml
[dependencies]
rlogger = "0.2.0"
```

```rust
use rlogger::{Config, Fields, InstanceConfig, Runtime, Value, info};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let runtime = Runtime::new(Config::new(std::env::temp_dir().join("my-app")))?;
let logger = runtime.instance(InstanceConfig::new("backend"))?;
let operation = logger.with_context(&Fields::from([
    ("call_id".into(), Value::from("c1")),
]))?;
info!(operation, "Starting request {}", 1);
runtime.flush()?;
let report = runtime.shutdown()?;
assert_eq!(report.totals.accepted, report.totals.written);
# Ok(())
# }
```

`Config::new(directory)` takes the application's parent directory. The writer
creates `directory/rlogger/<day>/<instance>/`. `Runtime::instance` registers
an instance, and cloned `Logger` handles share it. `with_context` creates an
immutable context snapshot. Application, instance, operation, and event fields
override one another in that order. Fields are sorted; the last value for a
key wins. Reserved internal fields and non-finite floating values are rejected.

## Emission and ordering

`emit` and `emit_args` return `Accepted(sequence)`, `Filtered`, or
`Refused(reason)`. Refusals cover full capacity, oversize events, closed or
failed runtimes, invalid fields, and unknown destinations. The `trace!`,
`debug!`, `info!`, `warn!`, and `error!` macros capture module, file, and
line, and avoid evaluating formatting arguments for filtered events.

Admission serializes capture, sequence assignment, and queue insertion. The
worker projects that order into each destination file; separate files have no
global flush order. Adjacent equivalent events can be grouped (for example,
`A A B A A` becomes `A x2 / B / A x2`). Group sequence bounds do not
reconstruct every occurrence. Emission performs no file I/O and does not wait
for capacity, although the admission mutex can be contended. Formatting code
supplied by the caller can still allocate or be expensive.

Declare destinations and module-prefix routes in `InstanceConfig` before
registering an instance. Routing prefers an explicit declared destination,
then the longest `::` module prefix, then a destination named after the crate,
then the first destination. Names use lowercase ASCII letters, digits,
underscore, or hyphen (up to 64 bytes); fields cannot create file paths.

## Files and lifecycle

The writer creates hourly `.active.log` segments under
`rlogger/<day>/<instance>/`, then flushes, closes, and publishes them as
`.log` at the end of the hour, on writer eviction, or during a healthy
shutdown. Finalized files are never reopened; a late capture gets a new
segment. `Runtime::log_root()` returns the storage root and `run_id()`
identifies this runtime's segments. Local capture date and UTC hour interval
distinguish repeated daylight-saving hours. Keep the process time zone stable
during a run.

The text format is versioned `RLOG/1`: control characters are escaped and
Unicode is retained. `LATENCY` measures monotonic capture to line preparation,
excluding write, flush, and display. The optional local reader can recover
abandoned active files under a system lease and exposes explicit file and
directory actions. It is a separate program in the
[repository](https://github.com/simonbdbc/rlogger).

Defaults include 8,192 event slots, 32 MiB of credits, an 8 KiB pessimistic
reservation per event, 32 instances, 128 destinations, and 32 writers with
64 KiB buffers. Groups close within 200 ms and file buffers normally flush
every 100 ms. These are resource settings, not a guaranteed display latency;
blocked disks and contention can delay them. Transient copies, metadata, and
allocator overhead add to the credit budget.

`stats()` and `Logger::counters()` expose accepted, written, filtered, and
refused events. `written` counts occurrences confirmed after flush, while
`pending = accepted - written`. After a write failure some bytes may be
visible without confirmation; partial writes are not replayed.
`Runtime::flush()` is a blocking global barrier without `fsync`.
`shutdown()` closes admission, drains, flushes, and joins the worker;
`ShutdownError.report` retains a partial report after a fail-stop error.
Dropping the runtime also drains, but cannot report errors. Use
`spawn_blocking` when flushing or shutting down from an async application.

## Optional `log` facade

Enable `rlogger = { version = "0.2.0", features = ["log"] }` and add
`log = "0.4"` to the application. `LogAdapter::new(Arc<Runtime>, Logger)`
implements `log::Log` for one RLOGGER instance. The application registers
it with `log::set_boxed_logger` and sets its maximum level; RLOGGER never
claims the process-global facade on its own. See the compiled
[example](examples/log_facade.rs).

An exact `log` target matching a declared destination selects that file;
otherwise the instance's module routes apply. The adapter uses static module
and file metadata when available. Dynamically borrowed locations fall back
to empty strings. Structured `log` key-value pairs are not copied in 0.2.0.
`log::Log::flush` forwards to the runtime but cannot return an error; use
`Runtime::flush` or `stats` for explicit results. The global adapter keeps
an `Arc<Runtime>` alive, so call `Runtime::shutdown` explicitly before exit.
The `log` facade permits only one global logger per process.

## Optional `tracing` layer

Enable `features = ["tracing"]` and compose
`tracing_subscriber::registry().with(LoggerLayer::new(logger))`. No global
subscriber is installed implicitly. `log_instance = "name"` selects another
pre-registered instance added through `with_instance`; an unknown name is
refused. Updated spans, explicit parents, and instrumented futures are
supported. Do not hold an entered span guard across `await`.

The adapter bounds its own storage to 1,024 spans, 64 simultaneous captures,
32 levels of depth, and 8 KiB of fields per capture. Subscriber-owned storage
is separate.

## Verify locally

```sh
cargo test --all-features
cargo test --features log
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo package
cargo run --example native -- /tmp/rlogger-native
cargo run --features log --example log_facade
cargo run --features tracing --example tracing_workflow
```

The examples use synthetic events and no network service. The repository also
contains external-consumer package checks and end-to-end reader tests. macOS
Apple Silicon is the locally measured platform; CI checks the library on Linux
as well. There is no promise of power-loss durability or global order across
processes.
