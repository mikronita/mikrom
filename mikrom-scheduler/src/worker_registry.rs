use crate::scheduler::ipam::Ipam;
use sqlx::PgPool;
use tonic::transport::Channel;

pub use crate::metrics::HostMetrics;

#[derive(Clone, Debug)]
pub struct Worker {
    pub host_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub agent_port: u16,
    pub bridge_ip: String,
    pub ipam: Ipam,
    pub channel: Option<Channel>,
    pub metrics: Option<HostMetrics>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

#[derive(Clone)]
pub struct WorkerRegistry {
    pool: PgPool,
}

impl WorkerRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn register(
        &self,
        host_id: String,
        hostname: String,
        ip_address: String,
        agent_port: u16,
        bridge_ip: String,
    ) -> Result<bool, sqlx::Error> {
        let now = chrono::Utc::now().timestamp();

        // Remove any stale worker with the same hostname but different host_id
        sqlx::query("DELETE FROM workers WHERE hostname = $1 AND id != $2")
            .bind(&hostname)
            .bind(&host_id)
            .execute(&self.pool)
            .await?;

        // Upsert the worker
        sqlx::query(
            r#"
            INSERT INTO workers (id, hostname, ip_address, agent_port, bridge_ip, last_heartbeat, registered_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO UPDATE SET
                hostname = EXCLUDED.hostname,
                ip_address = EXCLUDED.ip_address,
                agent_port = EXCLUDED.agent_port,
                bridge_ip = EXCLUDED.bridge_ip,
                last_heartbeat = EXCLUDED.last_heartbeat
            "#
        )
        .bind(&host_id)
        .bind(&hostname)
        .bind(&ip_address)
        .bind(agent_port as i32)
        .bind(&bridge_ip)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    pub async fn unregister(&self, host_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM workers WHERE id = $1")
            .bind(host_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_metrics(
        &self,
        host_id: &str,
        metrics: HostMetrics,
    ) -> Result<bool, sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let metrics_json = serde_json::to_value(metrics).unwrap_or_default();

        let result = sqlx::query(
            r#"
            UPDATE workers
            SET metrics = $1, last_heartbeat = $2, status = 'Online'
            WHERE id = $3
            "#,
        )
        .bind(metrics_json)
        .bind(now)
        .bind(host_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_worker(&self, host_id: &str) -> Result<Option<Worker>, sqlx::Error> {
        use sqlx::Row;
        let row = sqlx::query(
            "SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat FROM workers WHERE id = $1"
        )
        .bind(host_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
            let metrics: Option<HostMetrics> =
                metrics_val.and_then(|m| serde_json::from_value(m).ok());
            Ok(Some(Worker {
                host_id: r.get("id"),
                hostname: r.get("hostname"),
                ip_address: r.get("ip_address"),
                agent_port: r.get::<i32, _>("agent_port") as u16,
                bridge_ip: r.get::<String, _>("bridge_ip").clone(),
                ipam: Ipam::new(self.pool.clone(), r.get("id"), r.get("bridge_ip")),
                channel: None,
                metrics,
                registered_at: r.get("registered_at"),
                last_heartbeat: r.get("last_heartbeat"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_workers(&self) -> Result<Vec<Worker>, sqlx::Error> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat FROM workers"
        )
        .fetch_all(&self.pool)
        .await?;

        let workers = rows
            .into_iter()
            .map(|r| {
                let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
                let metrics: Option<HostMetrics> =
                    metrics_val.and_then(|m| serde_json::from_value(m).ok());
                Worker {
                    host_id: r.get("id"),
                    hostname: r.get("hostname"),
                    ip_address: r.get("ip_address"),
                    agent_port: r.get::<i32, _>("agent_port") as u16,
                    bridge_ip: r.get::<String, _>("bridge_ip").clone(),
                    ipam: Ipam::new(self.pool.clone(), r.get("id"), r.get("bridge_ip")),
                    channel: None,
                    metrics,
                    registered_at: r.get("registered_at"),
                    last_heartbeat: r.get("last_heartbeat"),
                }
            })
            .collect();

        Ok(workers)
    }

    pub async fn get_available_workers(&self) -> Result<Vec<Worker>, sqlx::Error> {
        use sqlx::Row;
        let now = chrono::Utc::now().timestamp();
        let threshold = now - 30; // 30 seconds staleness threshold

        let rows = sqlx::query(
            r#"
            SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat
            FROM workers
            WHERE metrics IS NOT NULL AND last_heartbeat > $1 AND status = 'Online'
            "#
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;

        let workers = rows
            .into_iter()
            .map(|r| {
                let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
                let metrics: Option<HostMetrics> =
                    metrics_val.and_then(|m| serde_json::from_value(m).ok());
                Worker {
                    host_id: r.get("id"),
                    hostname: r.get("hostname"),
                    ip_address: r.get("ip_address"),
                    agent_port: r.get::<i32, _>("agent_port") as u16,
                    bridge_ip: r.get::<String, _>("bridge_ip").clone(),
                    ipam: Ipam::new(self.pool.clone(), r.get("id"), r.get("bridge_ip")),
                    channel: None,
                    metrics,
                    registered_at: r.get("registered_at"),
                    last_heartbeat: r.get("last_heartbeat"),
                }
            })
            .collect();

        Ok(workers)
    }

    pub async fn is_registered(&self, host_id: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT 1 as one FROM workers WHERE id = $1")
            .bind(host_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    pub async fn get_metrics(&self, host_id: &str) -> Result<Option<HostMetrics>, sqlx::Error> {
        use sqlx::Row;
        let row = sqlx::query("SELECT metrics FROM workers WHERE id = $1")
            .bind(host_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(r) = row {
            let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
            let metrics: Option<HostMetrics> =
                metrics_val.and_then(|m| serde_json::from_value(m).ok());
            Ok(metrics)
        } else {
            Ok(None)
        }
    }
}
