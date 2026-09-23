use rlogger::*;
use tracing::Instrument;
use tracing_subscriber::prelude::*;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new(Config::new(
        std::env::temp_dir().join("ordered-tracing-demo"),
    ))?;
    let logger = runtime.instance(InstanceConfig::new("agents"))?;
    let subscriber = tracing_subscriber::registry().with(LoggerLayer::new(logger.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);
    info!(logger, "Native start");
    async {
        tracing::info!(call_id = "c1", "Début");
        tokio::task::yield_now().await;
        tracing::info!("Fin");
    }
    .instrument(tracing::info_span!("workflow", workflow_id = "w1"))
    .await;
    let report = tokio::task::spawn_blocking(move || runtime.shutdown()).await??;
    assert_eq!(report.totals.accepted, 3);
    assert_eq!(report.totals.written, 3);
    Ok(())
}
