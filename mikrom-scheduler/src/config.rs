use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SchedulerConfig {
    pub database_url: String,
    pub nats_url: String,

    #[serde(default = "default_http_port")]
    pub http_port: u16,

    #[serde(default = "default_router_idle_timeout_secs")]
    pub router_idle_timeout_secs: i64,

    #[serde(default = "default_worker_stale_threshold_secs")]
    pub worker_stale_threshold_secs: i64,

    #[serde(default = "default_restore_retry_backoff_secs")]
    pub restore_retry_backoff_secs: i64,

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

fn default_http_port() -> u16 {
    5003
}

fn default_database_max_connections() -> u32 {
    10
}

fn default_router_idle_timeout_secs() -> i64 {
    900
}

fn default_worker_stale_threshold_secs() -> i64 {
    60
}

fn default_restore_retry_backoff_secs() -> i64 {
    3600
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
                "postgres://[::1]/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://[::1]:4222".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.http_port, 5003);
        assert_eq!(config.router_idle_timeout_secs, 900);
    }

    #[test]
    fn loads_router_idle_timeout_from_env() {
        let config: SchedulerConfig = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://[::1]/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://[::1]:4222".to_string()),
            ("ROUTER_IDLE_TIMEOUT_SECS".to_string(), "120".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.router_idle_timeout_secs, 120);
    }

    #[test]
    fn defaults_restore_retry_backoff_and_worker_stale_threshold() {
        let config: SchedulerConfig = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://[::1]/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://[::1]:4222".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.worker_stale_threshold_secs, 60);
        assert_eq!(config.restore_retry_backoff_secs, 3600);
    }
}
