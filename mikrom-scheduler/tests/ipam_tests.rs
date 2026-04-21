#[cfg(test)]
mod tests {
    use mikrom_scheduler::scheduler::ipam::Ipam;

    #[test]
    fn test_large_network_10_8() {
        // Test with 10.0.0.0/8
        let ipam = Ipam::new("10.0.0.1/8");

        let a1 = ipam.allocate().expect("Should allocate IP");
        assert_eq!(a1.ip, "10.0.0.2");

        let a2 = ipam.allocate().expect("Should allocate IP");
        assert_eq!(a2.ip, "10.0.0.3");
    }

    #[test]
    fn test_ipam_boundary_conditions() {
        let ipam = Ipam::new("10.0.0.1/30"); // .0 net, .1 gw, .2 host, .3 bcast. Available: .2

        assert_eq!(ipam.allocate().map(|a| a.ip), Some("10.0.0.2".to_string()));
        assert_eq!(ipam.allocate(), None); // Exhausted
    }

    #[test]
    fn test_ipam_release_logic() {
        let ipam = Ipam::new("10.0.0.1/30");

        let a1 = ipam.allocate().unwrap();
        assert_eq!(ipam.allocate(), None);

        ipam.release(&a1.ip);
        let a_new = ipam.allocate().unwrap();
        assert_eq!(a_new.ip, a1.ip);
    }
}
