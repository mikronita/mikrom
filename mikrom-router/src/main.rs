fn main() -> anyhow::Result<()> {
    let config = mikrom_router::app::config::RouterConfig::load()?;
    mikrom_router::app::bootstrap::run(&config)
}
