use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SchedulerConfig {
    pub database_url: String,
    pub nats_url: String,

    #[serde(default = "default_router_idle_timeout_secs")]
    pub router_idle_timeout_secs: i64,

    #[serde(default = "default_database_max_connections")]
    pub database_max_connections: u32,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_certs_dir")]
    pub certs_dir: String,
}

fn default_use_tls() -> bool {
    false
}

fn default_database_max_connections() -> u32 {
    10
}

fn default_router_idle_timeout_secs() -> i64 {
    900
}

fn default_certs_dir() -> String {
    "/certs/scheduler".to_string()
}

impl SchedulerConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Self>().map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::SchedulerConfig;

    #[test]
    fn defaults_router_idle_timeout_to_fifteen_minutes() {
        let config: SchedulerConfig = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://localhost/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://localhost:4222".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.router_idle_timeout_secs, 900);
    }

    #[test]
    fn loads_router_idle_timeout_from_env() {
        let config: SchedulerConfig = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://localhost/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://localhost:4222".to_string()),
            ("ROUTER_IDLE_TIMEOUT_SECS".to_string(), "120".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.router_idle_timeout_secs, 120);
    }
}
