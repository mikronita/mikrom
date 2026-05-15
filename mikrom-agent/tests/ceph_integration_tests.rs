use mikrom_agent::ceph::CephRbd;
use uuid::Uuid;

#[tokio::test]
async fn test_ceph_rbd_lifecycle_native() {
    if !std::path::Path::new("/etc/ceph/ceph.conf").exists() {
        println!("Skipping Ceph test: /etc/ceph/ceph.conf not found");
        return;
    }
    let pool = "rbd";
    let volume_id = format!("test-vol-{}", Uuid::new_v4());
    let snapshot_name = "snap1";
    let size_mib = 128;

    // 1. Create Volume
    println!("Creating volume: {}", volume_id);
    CephRbd::create_volume(pool, &volume_id, size_mib)
        .await
        .expect("Failed to create volume");
    assert!(
        CephRbd::exists(pool, &volume_id).await,
        "Volume should exist after creation"
    );

    // 2. Create Snapshot
    println!("Creating snapshot: {}", snapshot_name);
    CephRbd::create_snapshot(pool, &volume_id, snapshot_name)
        .await
        .expect("Failed to create snapshot");

    // 3. Restore Snapshot
    println!("Restoring snapshot: {}", snapshot_name);
    CephRbd::restore_snapshot(pool, &volume_id, snapshot_name)
        .await
        .expect("Failed to restore snapshot");

    // 4. Clone Volume from Snapshot
    let clone_id = format!("test-vol-clone-{}", Uuid::new_v4());
    println!(
        "Cloning volume {}@{} to {}",
        volume_id, snapshot_name, clone_id
    );
    CephRbd::clone_volume(pool, &volume_id, snapshot_name, &clone_id)
        .await
        .expect("Failed to clone volume");
    assert!(
        CephRbd::exists(pool, &clone_id).await,
        "Cloned volume should exist"
    );

    // 5. Cleanup Clone
    CephRbd::delete_volume(pool, &clone_id)
        .await
        .expect("Failed to delete clone");

    // 6. Delete Snapshot
    println!("Deleting snapshot: {}", snapshot_name);
    CephRbd::delete_snapshot(pool, &volume_id, snapshot_name)
        .await
        .expect("Failed to delete snapshot");

    // 7. Delete Volume
    println!("Deleting volume: {}", volume_id);
    CephRbd::delete_volume(pool, &volume_id)
        .await
        .expect("Failed to delete volume");
    assert!(
        !CephRbd::exists(pool, &volume_id).await,
        "Volume should not exist after deletion"
    );
}

#[tokio::test]
async fn test_ceph_restore_busy_image_failure() {
    if !std::path::Path::new("/etc/ceph/ceph.conf").exists() {
        println!("Skipping Ceph test: /etc/ceph/ceph.conf not found");
        return;
    }
    let pool = "rbd";
    let volume_id = format!("test-busy-vol-{}", Uuid::new_v4());
    let snapshot_name = "snap1";
    let size_mib = 128;

    CephRbd::create_volume(pool, &volume_id, size_mib)
        .await
        .unwrap();
    CephRbd::create_snapshot(pool, &volume_id, snapshot_name)
        .await
        .unwrap();

    // Simulate busy image by mapping it
    if unsafe { libc::getuid() } == 0 {
        let dev_path = CephRbd::map_volume(pool, &volume_id)
            .await
            .expect("Failed to map volume");

        // Attempt to restore - should fail
        let res = CephRbd::restore_snapshot(pool, &volume_id, snapshot_name).await;
        assert!(res.is_err(), "Restore should fail for mapped volume");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("busy") || err.contains("Stop the VM"),
            "Error message should mention busy/Stop the VM"
        );

        CephRbd::unmap_volume(&dev_path).await.unwrap();
    } else {
        println!("Skipping busy test (not root)");
    }

    CephRbd::delete_snapshot(pool, &volume_id, snapshot_name)
        .await
        .unwrap();
    CephRbd::delete_volume(pool, &volume_id).await.unwrap();
}
