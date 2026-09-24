use local_logs_server::{Options, start};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tokio::process::Command;

type SourceStamp = BTreeMap<PathBuf, (u64, u128)>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = Options::default();
    if let Ok(port) = std::env::var("LOCAL_LOGS_PORT") {
        options.port = port.parse()?;
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => options.port = args.next().ok_or("Missing port")?.parse()?,
            "--dist" => options.dist = args.next().ok_or("Missing dist")?.into(),
            "--dev" => options.dev_reload = true,
            "--poll-ms" => {
                options.poll = Duration::from_millis(
                    args.next()
                        .ok_or("Missing poll")?
                        .parse::<u64>()?
                        .clamp(10, 5000),
                )
            }
            "--help" => {
                println!(
                    "local-logs-server [--port 4317] [--dist front-react-logger/dist] [--poll-ms 250] [--dev]"
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }

    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../front-react-logger");
    let dev_slots = [
        frontend.join(".local-logs-dev/slot-0"),
        frontend.join(".local-logs-dev/slot-1"),
    ];
    if options.dev_reload {
        if !export_web(&frontend, &dev_slots[0]).await? {
            return Err(io::Error::other("Initial Expo export failed").into());
        }
        options.dist = dev_slots[0].clone();
    }

    let service = start(options.clone()).await?;
    println!("Local Logs : {}\nArrêt : Ctrl+C", service.origin);
    let shutdown = shutdown();
    tokio::pin!(shutdown);
    if options.dev_reload {
        let mut stamp = source_stamp(&frontend)?;
        let mut slot = 0;
        let mut interval = tokio::time::interval(Duration::from_millis(400));
        loop {
            tokio::select! {
                result = &mut shutdown => { result?; break; }
                _ = interval.tick() => {
                    let current = source_stamp(&frontend)?;
                    if current == stamp { continue; }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let stable = source_stamp(&frontend)?;
                    if stable == stamp { continue; }
                    stamp = stable;
                    let next = 1 - slot;
                    println!("Source Expo modifiée : export en cours…");
                    match export_web(&frontend, &dev_slots[next]).await {
                        Ok(true) => match service.reload_dist(&dev_slots[next]) {
                            Ok(()) => { slot = next; println!("Export prêt : navigateur actualisé automatiquement."); }
                            Err(error) => eprintln!("Export illisible : {error}"),
                        },
                        Ok(false) => eprintln!("Export Expo échoué ; corrigez la source pour réessayer."),
                        Err(error) => eprintln!("Export Expo indisponible : {error}"),
                    }
                }
            }
        }
    } else {
        shutdown.await?;
    }
    service.close().await;
    Ok(())
}

async fn export_web(frontend: &Path, output: &Path) -> io::Result<bool> {
    let status = Command::new(frontend.join("node_modules/.bin/expo"))
        .args(["export", "--platform", "web", "--output-dir"])
        .arg(output)
        .current_dir(frontend)
        .env("EXPO_NO_TELEMETRY", "1")
        .status()
        .await?;
    Ok(status.success())
}

fn source_stamp(frontend: &Path) -> io::Result<SourceStamp> {
    let mut files = BTreeMap::new();
    for directory in ["app", "src", "shared", "assets", "public"] {
        stamp_path(&frontend.join(directory), &mut files)?;
    }
    for file in [
        "app.json",
        "babel.config.js",
        "metro.config.js",
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "expo-env.d.ts",
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ] {
        stamp_path(&frontend.join(file), &mut files)?;
    }
    Ok(files)
}

fn stamp_path(path: &Path, files: &mut SourceStamp) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            stamp_path(&entry?.path(), files)?;
        }
    } else if metadata.is_file() {
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        files.insert(path.to_path_buf(), (metadata.len(), modified));
    }
    Ok(())
}

async fn shutdown() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result=tokio::signal::ctrl_c()=>result, _=term.recv()=>Ok(()) }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
