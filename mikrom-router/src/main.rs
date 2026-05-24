fn main() -> anyhow::Result<()> {
    let config = mikrom_router::config::RouterConfig::load()?;
    mikrom_router::bootstrap::run(&config)
}
