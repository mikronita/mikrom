use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    mikrom_dns::run().await
}
