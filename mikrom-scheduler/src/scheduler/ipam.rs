use sqlx::PgPool;
use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct Ipam {
    pool: PgPool,
    worker_id: String,
    bridge_ip: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub ip: String,
    pub gateway: String,
    pub mac: String,
}

impl Ipam {
    #[must_use]
    pub fn new(pool: PgPool, worker_id: String, bridge_ip: String) -> Self {
        Self {
            pool,
            worker_id,
            bridge_ip,
        }
    }

    pub async fn allocate(&self) -> Result<Option<Allocation>, sqlx::Error> {
        let (ip_str, prefix_str) = self
            .bridge_ip
            .split_once('/')
            .unwrap_or((&self.bridge_ip, "24"));
        let prefix: u32 = prefix_str.trim().parse().unwrap_or(24);
        let gateway: Ipv4Addr = ip_str.trim().parse().unwrap_or(Ipv4Addr::new(10, 0, 0, 1));

        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32 - prefix)
        };

        let base = Ipv4Addr::from(u32::from(gateway) & mask);
        let base_u32 = u32::from(base);
        let gw_u32 = u32::from(gateway);

        let max_hosts = if prefix == 0 {
            u32::MAX - 1
        } else if prefix >= 32 {
            1
        } else {
            (1u32 << (32 - prefix)).saturating_sub(2)
        };

        let scan_limit = std::cmp::min(max_hosts, 1024);

        // Use a transaction for atomic allocation
        let mut tx = self.pool.begin().await?;

        // Get currently allocated IPs for this worker
        let allocated_ips: std::collections::HashSet<String> =
            sqlx::query("SELECT ip_address FROM ip_allocations WHERE worker_id = $1")
                .bind(&self.worker_id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|r| {
                    use sqlx::Row;
                    r.get("ip_address")
                })
                .collect();

        for offset in 2..=scan_limit {
            let candidate = Ipv4Addr::from(base_u32 + offset);
            let candidate_str = candidate.to_string();

            if u32::from(candidate) == gw_u32 {
                continue;
            }

            if !allocated_ips.contains(&candidate_str) {
                let o = candidate.octets();
                let mac = format!("AA:FC:{:02X}:{:02X}:{:02X}:{:02X}", o[0], o[1], o[2], o[3]);

                // Insert the allocation
                sqlx::query(
                    "INSERT INTO ip_allocations (ip_address, worker_id, mac_address) VALUES ($1, $2, $3)"
                )
                .bind(&candidate_str)
                .bind(&self.worker_id)
                .bind(&mac)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;

                return Ok(Some(Allocation {
                    ip: candidate_str,
                    gateway: gateway.to_string(),
                    mac,
                }));
            }
        }

        tx.rollback().await?;
        Ok(None)
    }

    pub fn netmask(&self) -> String {
        let (_ip_str, prefix_str) = self
            .bridge_ip
            .split_once('/')
            .unwrap_or((&self.bridge_ip, "24"));
        let prefix: u32 = prefix_str.trim().parse().unwrap_or(24);

        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32 - prefix)
        };
        Ipv4Addr::from(mask).to_string()
    }

    pub async fn release(&self, ip_str: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM ip_allocations WHERE ip_address = $1 AND worker_id = $2")
            .bind(ip_str)
            .bind(&self.worker_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
