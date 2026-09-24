# RLOGGER

RLOGGER is a bounded, ordered file logger for Rust. This repository also contains
a local Rust HTTP/WebSocket companion and an Expo/gluestack web reader as a
local usage example. The logger does not start a server or an async runtime.

The Rust crate is in [lib-rust-logger](lib-rust-logger/README.md). Use it on its own:

```toml
[dependencies]
rlogger = "0.2.0"
```

```rust
use rlogger::{Config, InstanceConfig, Runtime, info};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let runtime = Runtime::new(Config::new(std::env::temp_dir().join("my-app")))?;
let logger = runtime.instance(InstanceConfig::new("backend"))?;
info!(logger, "Ready");
runtime.shutdown()?;
# Ok(())
# }
```

The optional `log` and `tracing` adapters are documented in the
[crate guide](lib-rust-logger/README.md). Rust 1.95 or newer is required.
The whole repository is licensed under [MIT](LICENSE).

## Run the local reader

The reader is optional and runs on the same machine. Its backend and project
tools are Rust; Node 24 is used only to build and test the Expo frontend.

```sh
cargo run --manifest-path lib-rust-logger/Cargo.toml --example native -- /tmp/rlogger-demo
cd front-react-logger
npm ci
npm run build
cd ..
cargo run --manifest-path local-logs-server/Cargo.toml --release
```

Open [Local Logs](http://127.0.0.1:4317), select **Journaux RLOGGER** in the side
menu, enter `/tmp/rlogger-demo/rlogger`, and select a file. Choose **Journaux
externes** for another application's log directory; this view has no automatic
maintenance or RLOGGER format warning. New content appears without reloading.
The demo prints its
log root and shutdown report. Stop the reader with Ctrl+C; the library and log
files remain independent.

For frontend development, run `npm run dev` from `front-react-logger/`. The Rust
companion watches Expo sources, reruns the web export on changes and reloads the
browser page automatically. This is a full page reload; no Node server is used.
`npm run preview` keeps the one-off build and static reader workflow.

For an async producer example, run:

```sh
cargo run --manifest-path lib-rust-logger/Cargo.toml --example workflow -- /tmp/rlogger-workflow
```

To try the reader without a producer, run
`cargo run --manifest-path local-logs-server/Cargo.toml --example fixtures` and
open the private directory printed by that command under **Journaux externes**.
Add `-- --managed` to create a RLOGGER storage root for the management demo.

The reader shows file and directory sizes. Explicit file and directory download
and deletion follow the host filesystem permissions, including for generic log
roots; directory downloads use uncompressed TAR. Hidden entries are omitted
from the tree but included in parent sizes and recursive actions. RLOGGER's
automatic recovery and empty-day cleanup require an eligible private RLOGGER
root and the RLOGGER mode. External mode disables automatic maintenance in the
Rust companion even when pointed at a valid RLOGGER root. Each mode remembers
and automatically opens its own last directory when selected or restored at
startup. External log paths are entered locally and are not bundled with the
repository. See the [current action contract](docs/11-actions-fichiers-dossiers.md)
and [compatibility notes](docs/COMPATIBILITE.md).

## Develop and verify

```sh
cargo test --manifest-path lib-rust-logger/Cargo.toml --all-features
cargo test --manifest-path local-logs-server/Cargo.toml --all-features
cd front-react-logger && npm ci && npm run build && npm run test:e2e
```

The [CI workflow](.github/workflows/ci.yml) also checks Rust formatting,
Clippy, package verification and frontend types and formatting. See the
[reader guide](front-react-logger/README.md).
The detailed technical guides remain in French under [docs](docs/README.md).
Local measurement data and screenshots are excluded from
the public repository. Local validation does not by itself establish a GitHub
or crates.io release.
