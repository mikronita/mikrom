use aya::maps::HashMap;
use aya::programs::tc;
use aya::{Ebpf, include_bytes_aligned};
use aya_log::EbpfLogger;
use mikrom_agent_ebpf_common::{FirewallRule, NetworkStats};
use tracing::{info, warn};

pub struct EbpfManager {
    ebpf: Ebpf,
}

impl EbpfManager {
    pub fn load() -> anyhow::Result<Self> {
        let data =
            include_bytes_aligned!("../../../target/bpfel-unknown-none/release/mikrom-agent-ebpf");

        let mut ebpf = Ebpf::load(data)?;
        if let Err(e) = EbpfLogger::init(&mut ebpf) {
            warn!("failed to initialize eBPF logger: {}", e);
        }

        Ok(Self { ebpf })
    }

    pub fn attach_tc(&mut self, iface: &str) -> anyhow::Result<()> {
        let ingress: &mut tc::SchedClassifier = self
            .ebpf
            .program_mut("mikrom_ingress")
            .ok_or_else(|| anyhow::anyhow!("program mikrom_ingress not found"))?
            .try_into()?;
        ingress.load()?;
        ingress.attach(iface, aya::programs::tc::TcAttachType::Ingress)?;

        let egress: &mut tc::SchedClassifier = self
            .ebpf
            .program_mut("mikrom_egress")
            .ok_or_else(|| anyhow::anyhow!("program mikrom_egress not found"))?
            .try_into()?;
        egress.load()?;
        egress.attach(iface, aya::programs::tc::TcAttachType::Egress)?;

        info!("Attached eBPF TC filters to interface {}", iface);
        Ok(())
    }

    pub fn get_stats(&self, ifindex: u32) -> Option<NetworkStats> {
        let map = self.ebpf.map("STATS")?;
        let stats: HashMap<_, u32, NetworkStats> = HashMap::try_from(map).ok()?;
        stats.get(&ifindex, 0).ok()
    }

    pub fn update_rules(&mut self, ifindex: u32, rules: Vec<FirewallRule>) -> anyhow::Result<()> {
        let map = self
            .ebpf
            .map_mut("RULES")
            .ok_or_else(|| anyhow::anyhow!("map RULES not found"))?;
        let mut rules_map: HashMap<_, u32, FirewallRule> = HashMap::try_from(map)?;

        // 1. Clear existing rules for this ifindex (up to 16)
        for i in 0..16 {
            let key = (ifindex << 4) | i;
            let _ = rules_map.remove(&key);
        }

        // 2. Insert new rules
        for (i, rule) in rules.into_iter().take(16).enumerate() {
            let key = (ifindex << 4) | (i as u32);
            rules_map.insert(key, rule, 0)?;
        }

        Ok(())
    }
}
