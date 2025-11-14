use clap::Parser;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value_t = 17000)]
    pub control_port: u16,

    #[arg(long, default_value_t = 17001)]
    pub proxy_port: u16,

    #[arg(long, default_value_t = 18080)]
    pub public_port: u16,

    #[arg(long, default_value_t = 18081)]
    pub api_port: u16,

    /// Print client monitoring data
    #[arg(long)]
    pub monitor: bool,

    /// API key for authentication
    #[arg(long, default_value = "abc123")]
    pub api_key: String,

    /// Database URL for PostgreSQL connection
    #[arg(
        long,
        default_value = "postgres://username:password@localhost/database"
    )]
    pub database_url: String,

    /// Path to the certificate chain file
    #[arg(long, default_value = "cert.pem")]
    pub proxy_cert_chain_path: String,

    /// Path to the private key file
    #[arg(long, default_value = "key.pem")]
    pub proxy_private_key_path: String,

    /// Redis URL for caching
    #[arg(long, default_value = "redis://127.0.0.1:6379")]
    pub redis_url: String,

    #[arg(long, default_value = "localhost:9092")]
    pub bootstrap_server: String,
}
