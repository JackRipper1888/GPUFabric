use anyhow::Result;
use clap::Parser;
use gpuf_s::consumer;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "100")]
    pub batch_size: usize,

    #[arg(long, default_value = "5")]
    pub batch_timeout: u64,

    #[arg(
        long,
        default_value = "postgres://username:password@localhost/database"
    )]
    pub database_url: String,

    #[arg(long, default_value = "localhost:9092")]
    pub bootstrap_server: String,

    #[arg(long, default_value = "300")]
    pub offline_after_secs: i64,

    #[arg(long, default_value = "30")]
    pub sweep_interval_secs: u64,
}
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("gpuf-s=info".parse()?))
        .init();

    // Parse command line arguments
    let args = Args::try_parse()?;

    // Initialize database connection pool
    let db_pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&args.database_url)
        .await
    {
        Ok(pool) => pool,
        Err(e) => {
            error!("Failed to connect to database: {}", e);
            return Err(anyhow::anyhow!("Database connection failed"));
        }
    };

    let sweep_pool = db_pool.clone();
    let offline_after_secs = args.offline_after_secs;
    let sweep_interval_secs = args.sweep_interval_secs;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(sweep_interval_secs));
        loop {
            ticker.tick().await;

            let res = sqlx::query(
                "UPDATE \"public\".\"gpu_assets\" \n                 SET client_status = 'offline', updated_at = NOW() \n                 WHERE valid_status = 'valid' \n                   AND client_status <> 'offline' \n                   AND updated_at < (NOW() - ($1 * INTERVAL '1 second'))",
            )
            .bind(offline_after_secs)
            .execute(&sweep_pool)
            .await;

            match res {
                Ok(r) => {
                    let n = r.rows_affected();
                    if n > 0 {
                        info!(
                            "Sweeper marked {} clients offline (offline_after_secs={})",
                            n, offline_after_secs
                        );
                    }
                }
                Err(e) => {
                    error!("Sweeper failed to mark stale clients offline: {}", e);
                }
            }
        }
    });

    // Start the consumer service
    consumer::start_consumer_services(
        &args.bootstrap_server, // From your command line args
        "heartbeat-consumer-group",
        "client-heartbeats",
        db_pool,
        args.batch_size,    // Batch size
        args.batch_timeout, // Batch timeout in seconds
    )
    .await?;

    Ok(())
}
