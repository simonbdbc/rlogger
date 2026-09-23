use local_logs_server::{Options, start};
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
            "--poll-ms" => {
                options.poll = std::time::Duration::from_millis(
                    args.next()
                        .ok_or("Missing poll")?
                        .parse::<u64>()?
                        .clamp(10, 5000),
                )
            }
            "--help" => {
                println!(
                    "local-logs-server [--port 4317] [--dist front-react-logger/dist] [--poll-ms 250]"
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    let service = start(options).await?;
    println!("Local Logs : {}\nArrêt : Ctrl+C", service.origin);
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    service.close().await;
    Ok(())
}
