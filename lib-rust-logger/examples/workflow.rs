use rlogger::*;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ordered-logger-workflow"));
    let runtime = Runtime::new(Config::new(directory))?;
    println!("{}", runtime.log_root().display());
    let logger = runtime.instance(InstanceConfig::new("agents"))?;
    let mut tasks = Vec::new();
    for agent in 0..4u64 {
        let operation = logger.with_context(&Fields::from([
            ("workflow_id".into(), "demo".into()),
            ("agent_id".into(), agent.into()),
            ("call_id".into(), format!("call-{agent}").into()),
            ("parent".into(), "root".into()),
        ]))?;
        tasks.push(tokio::spawn(async move {
            let start = std::time::Instant::now();
            info!(operation, "Début appel synthétique");
            tokio::task::yield_now().await;
            info!(operation, "Premier token 🦀");
            match agent {
                2 => {
                    error!(operation, "Erreur synthétique");
                }
                3 => {
                    warn!(operation, "Annulation synthétique");
                }
                _ => {
                    info!(
                        operation,
                        "Fin appel duration_us={}",
                        start.elapsed().as_micros()
                    );
                }
            }
        }));
    }
    for task in tasks {
        task.await?;
    }
    let report = tokio::task::spawn_blocking(move || runtime.shutdown()).await??;
    assert_eq!(report.totals.accepted, report.totals.written);
    println!("{report:?}");
    Ok(())
}
