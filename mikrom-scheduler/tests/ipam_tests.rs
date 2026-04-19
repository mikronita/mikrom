#[cfg(test)]
mod tests {
    use mikrom_scheduler::scheduler::ipam::Ipam;

    #[test]
    fn test_large_network_10_8() {
        // Test con la red 10.0.0.0/8 empezando en un offset alto
        let ipam = Ipam::new("10.0.0.0/8", 65536, 16777214); // Empieza en 10.1.0.0

        let ip = ipam.allocate().expect("Should allocate IP");
        assert_eq!(ip, "10.1.0.0");

        let ip2 = ipam.allocate().expect("Should allocate IP");
        assert_eq!(ip2, "10.1.0.1");
    }

    #[test]
    fn test_ipam_boundary_conditions() {
        let ipam = Ipam::new("10.0.0.0/8", 16777214, 16777214);

        assert_eq!(ipam.allocate(), Some("10.255.255.254".to_string()));
        assert_eq!(ipam.allocate(), None); // Agotado
    }

    #[test]
    fn test_ipam_release_logic() {
        let ipam = Ipam::new("10.0.0.0/8", 100, 101);

        let ip1 = ipam.allocate().unwrap();
        let _ip2 = ipam.allocate().unwrap();
        assert_eq!(ipam.allocate(), None);

        ipam.release(&ip1);
        assert_eq!(ipam.allocate(), Some(ip1));
    }
}
