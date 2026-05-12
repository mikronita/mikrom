use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Worker {
    pub id: String,
    pub hostname: String,
    pub wireguard_pubkey: String,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<mikrom_proto::scheduler::WorkerInfo> for Worker {
    fn from(w: mikrom_proto::scheduler::WorkerInfo) -> Self {
        use chrono::TimeZone;
        let last_seen = Utc
            .timestamp_opt(w.last_heartbeat, 0)
            .single()
            .unwrap_or_default();
        Self {
            id: w.host_id,
            hostname: w.hostname,
            wireguard_pubkey: w.wireguard_pubkey,
            last_seen_at: last_seen,
            created_at: last_seen,
        }
    }
}
