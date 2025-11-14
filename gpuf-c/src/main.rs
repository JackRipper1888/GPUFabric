mod handle;
mod llm_engine;
mod util;
use anyhow::Result;
use clap::Parser;
use handle::WorkerHandle;
use tokio_rustls::rustls::crypto::aws_lc_rs;
use tracing::{debug, info};

#[tokio::main]
async fn main() -> Result<()> {
    let provider = aws_lc_rs::default_provider();
    provider.install_default().map_err(|e| {
        anyhow::anyhow!(
            "Failed to install default AWS provider: {:?}. This usually indicates a problem with the system encryption configuration.",
            e
        )
    })?;

    util::init_logging();

    let args = util::cmd::Args::parse().load_config()?;
    debug!("args: {:#?}", args);
    info!("Server address: {}:{}", args.server_addr, args.control_port);
    info!("Local service: {}:{}", args.local_addr, args.local_port);

    let worker = handle::new_worker(args).await;

    worker.login().await?;
    worker.handler().await?;
    Ok(())
}
