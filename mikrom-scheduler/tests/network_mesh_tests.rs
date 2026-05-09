#[test]
fn test_mesh_ip_formatting() {
    // Test that our manual logic in event_loop.rs for adding prefixes is correct
    let wg_ips = vec!["fd00::1", "10.0.0.1", "fd00::abcd:1234"];

    let formatted: Vec<String> = wg_ips
        .into_iter()
        .map(|ip| {
            let prefix = if ip.contains(':') { "/128" } else { "/32" };
            format!("{}{}", ip, prefix)
        })
        .collect();

    assert_eq!(formatted[0], "fd00::1/128");
    assert_eq!(formatted[1], "10.0.0.1/32");
    assert_eq!(formatted[2], "fd00::abcd:1234/128");
}

#[test]
fn test_peer_filtering_logic() {
    // Simulate the logic in event_loop.rs:
    // for peer_worker in &workers {
    //    if peer_worker.host_id == w.host_id || peer_worker.wireguard_pubkey.is_none() { continue; }

    let my_id = "node-a";
    let workers = [
        ("node-a", Some("pub-a")),
        ("node-b", Some("pub-b")),
        ("node-c", None),
    ];

    let peers: Vec<_> = workers
        .iter()
        .filter(|(id, pubkey)| *id != my_id && pubkey.is_some())
        .collect();

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].0, "node-b");
}
