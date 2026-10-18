use anyhow::Result;
use diary::{config::Config, init_tracing};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    init_tracing(&config.rust_log);
    diary::run(config).await
}
