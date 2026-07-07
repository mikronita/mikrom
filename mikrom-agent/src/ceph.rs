use anyhow::{Result, anyhow};
use librados_sys::{
    rados_conf_read_file, rados_connect, rados_create, rados_ioctx_create, rados_ioctx_destroy,
    rados_ioctx_t, rados_pool_create, rados_shutdown, rados_t,
};
use librbd_sys::{rbd_close, rbd_create, rbd_open, rbd_remove, rbd_snap_create};
use std::ffi::CString;
use std::ptr;
use tracing::{info, warn};

// Missing FFI declarations from sys crates
unsafe extern "C" {
    fn rados_application_enable(
        cluster: rados_t,
        pool_name: *const libc::c_char,
        app_name: *const libc::c_char,
        force: libc::c_int,
    ) -> libc::c_int;
    fn rbd_pool_init(io: rados_ioctx_t, force: libc::c_int) -> libc::c_int;
    fn rbd_clone(
        p_ioctx: rados_ioctx_t,
        p_name: *const libc::c_char,
        p_snapname: *const libc::c_char,
        c_ioctx: rados_ioctx_t,
        c_name: *const libc::c_char,
        features: u64,
        order: *mut libc::c_int,
    ) -> libc::c_int;
    fn rbd_snap_protect(
        image: librbd_sys::rbd_image_t,
        snapname: *const libc::c_char,
    ) -> libc::c_int;
    fn rbd_snap_unprotect(
        image: librbd_sys::rbd_image_t,
        snapname: *const libc::c_char,
    ) -> libc::c_int;
    fn rbd_snap_rollback(
        image: librbd_sys::rbd_image_t,
        snapname: *const libc::c_char,
    ) -> libc::c_int;
    fn rbd_update_features(
        image: librbd_sys::rbd_image_t,
        features: u64,
        enabled: libc::c_int,
    ) -> libc::c_int;
    fn rbd_watchers_list(
        image: librbd_sys::rbd_image_t,
        watchers: *mut *mut rbd_image_watcher_t,
        num_watchers: *mut libc::size_t,
    ) -> libc::c_int;
    fn rbd_watchers_list_cleanup(watchers: *mut rbd_image_watcher_t, num_watchers: libc::size_t);
    fn rbd_snap_list(
        image: librbd_sys::rbd_image_t,
        snaps: *mut *mut rbd_snap_info_t,
        num_snaps: *mut libc::c_int,
    ) -> libc::c_int;
    fn rbd_snap_list_end(snaps: *mut rbd_snap_info_t);
}

#[repr(C)]
struct rbd_image_watcher_t {
    addr: [libc::c_char; 256],
    id: i64,
    cookie: u64,
}

#[repr(C)]
struct rbd_snap_info_t {
    id: u64,
    size: u64,
    name: *mut libc::c_char,
}

pub struct CephRbd;

#[allow(async_fn_in_trait)]
pub trait StorageProvider: Send + Sync {
    async fn ensure_pool_exists(&self, pool: &str) -> Result<()>;
    async fn create_volume(&self, pool: &str, name: &str, size_mib: i32) -> Result<()>;
    async fn delete_volume(&self, pool: &str, name: &str) -> Result<()>;
    async fn create_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()>;
    async fn delete_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()>;
    async fn restore_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()>;
    async fn clone_volume(
        &self,
        pool: &str,
        source_name: &str,
        snapshot_name: &str,
        target_name: &str,
    ) -> Result<()>;
    async fn exists(&self, pool: &str, name: &str) -> bool;
    async fn map_volume(&self, pool: &str, name: &str) -> Result<String>;
    async fn unmap_volume(&self, device_path: &str) -> Result<()>;
}

/// RAII wrapper for RADOS cluster handle
struct RadosCluster(rados_t);
unsafe impl Send for RadosCluster {}
unsafe impl Sync for RadosCluster {}
impl Drop for RadosCluster {
    fn drop(&mut self) {
        unsafe { rados_shutdown(self.0) };
    }
}

/// RAII wrapper for RADOS IO Context
struct IoCtx {
    handle: rados_ioctx_t,
    _cluster: std::sync::Arc<RadosCluster>,
}
impl Drop for IoCtx {
    fn drop(&mut self) {
        unsafe { rados_ioctx_destroy(self.handle) };
    }
}

impl CephRbd {
    fn connect(pool: &str) -> Result<IoCtx> {
        unsafe {
            let mut cluster: rados_t = ptr::null_mut();
            let id = CString::new("admin").map_err(|e| anyhow!("Invalid admin name: {}", e))?;

            let ret = rados_create(&mut cluster, id.as_ptr());
            if ret < 0 {
                return Err(anyhow!("Failed to create rados handle: {}", ret));
            }
            let cluster = std::sync::Arc::new(RadosCluster(cluster));

            let config =
                CString::new("/etc/ceph/ceph.conf").map_err(|e| anyhow!("Invalid path: {}", e))?;
            let ret = rados_conf_read_file(cluster.0, config.as_ptr());
            if ret < 0 {
                return Err(anyhow!("Failed to read ceph.conf: {}", ret));
            }

            let ret = rados_connect(cluster.0);
            if ret < 0 {
                return Err(anyhow!("Failed to connect to ceph cluster: {}", ret));
            }

            let mut ioctx: rados_ioctx_t = ptr::null_mut();
            let pool_name_c =
                CString::new(pool).map_err(|e| anyhow!("Invalid pool name: {}", e))?;
            let ret = rados_ioctx_create(cluster.0, pool_name_c.as_ptr(), &mut ioctx);

            if ret < 0 {
                return Err(anyhow!("Failed to open pool {}: {}", pool, ret));
            }

            Ok(IoCtx {
                handle: ioctx,
                _cluster: cluster,
            })
        }
    }

    /// Provision a pool if it does not exist. Separated from connect to avoid side effects in every connection.
    pub async fn ensure_pool_exists(pool: &str) -> Result<()> {
        unsafe {
            let mut cluster: rados_t = ptr::null_mut();
            let id = CString::new("admin").map_err(|e| anyhow!("Invalid admin name: {}", e))?;
            let ret = rados_create(&mut cluster, id.as_ptr());
            if ret < 0 {
                return Err(anyhow!("Failed to create rados handle: {}", ret));
            }
            let cluster = RadosCluster(cluster);

            let config =
                CString::new("/etc/ceph/ceph.conf").map_err(|e| anyhow!("Invalid path: {}", e))?;
            let conf_ret = rados_conf_read_file(cluster.0, config.as_ptr());
            if conf_ret < 0 {
                warn!("Failed to read ceph.conf for pool existence check: {}", conf_ret);
            }
            let conn_ret = rados_connect(cluster.0);
            if conn_ret < 0 {
                warn!("Failed to connect to ceph cluster for pool existence check: {}", conn_ret);
            }

            let mut ioctx: rados_ioctx_t = ptr::null_mut();
            let pool_name_c =
                CString::new(pool).map_err(|e| anyhow!("Invalid pool name: {}", e))?;
            let ret = rados_ioctx_create(cluster.0, pool_name_c.as_ptr(), &mut ioctx);

            if ret == -2 {
                // ENOENT: Pool not found
                info!("Pool {} not found, creating it natively...", pool);
                let ret_create = rados_pool_create(cluster.0, pool_name_c.as_ptr());
                if ret_create < 0 {
                    return Err(anyhow!("Failed to create pool {}: {}", pool, ret_create));
                }

                // Enable 'rbd' application on the pool
                let app_name =
                    CString::new("rbd").map_err(|e| anyhow!("Invalid app name: {}", e))?;
                let ret_app =
                    rados_application_enable(cluster.0, pool_name_c.as_ptr(), app_name.as_ptr(), 0);
                if ret_app < 0 {
                    warn!(
                        "Failed to enable 'rbd' application on pool {}: {}",
                        pool, ret_app
                    );
                }

                // Initialize pool for RBD
                let ret_io = rados_ioctx_create(cluster.0, pool_name_c.as_ptr(), &mut ioctx);
                if ret_io == 0 {
                    let ret_init = rbd_pool_init(ioctx, 0);
                    rados_ioctx_destroy(ioctx);
                    if ret_init < 0 {
                        warn!("Failed to initialize RBD pool {}: {}", pool, ret_init);
                    }
                }
            } else if ret == 0 {
                rados_ioctx_destroy(ioctx);
            }
            Ok(())
        }
    }

    pub async fn create_volume(pool: &str, name: &str, size_mib: i32) -> Result<()> {
        Self::ensure_pool_exists(pool).await?;

        info!(
            "Creating RBD volume (native FFI): {}/{} ({} MiB)",
            pool, name, size_mib
        );
        let io = Self::connect(pool)?;
        let name_c = CString::new(name).map_err(|e| anyhow!("Invalid volume name: {}", e))?;
        let size_bytes = (size_mib as u64) * 1024 * 1024;

        unsafe {
            let mut order = 0;
            let ret = rbd_create(io.handle, name_c.as_ptr(), size_bytes, &mut order);
            if ret < 0 {
                return Err(anyhow!("rbd_create failed with code {}", ret));
            }
        }
        Ok(())
    }

    pub async fn purge_snapshots(_pool: &str) -> Result<()> {
        // This is a placeholder as purging all requires an image name.
        // The trait method delete_volume calls this with name.
        Ok(())
    }

    pub async fn purge_image_snapshots(pool: &str, name: &str) -> Result<()> {
        info!(
            "Purging all snapshots for RBD image (FFI): {}/{}",
            pool, name
        );
        let io = Self::connect(pool)?;
        let name_c = CString::new(name)?;

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            if rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null()) < 0 {
                return Ok(()); // Image might already be gone
            }

            let mut snaps: *mut rbd_snap_info_t = ptr::null_mut();
            let mut num_snaps: libc::c_int = 0;

            if rbd_snap_list(image, &mut snaps, &mut num_snaps) >= 0 && !snaps.is_null() {
                let snap_slice = std::slice::from_raw_parts(snaps, num_snaps as usize);
                for snap in snap_slice {
                    let snap_name = if snap.name.is_null() {
                        "<null>".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(snap.name)
                            .to_string_lossy()
                            .into_owned()
                    };
                    let snap_ret = librbd_sys::rbd_snap_remove(image, snap.name);
                    if snap_ret < 0 {
                        warn!("Failed to remove snapshot {}: {}", snap_name, snap_ret);
                    }
                }
                rbd_snap_list_end(snaps);
            }

            rbd_close(image);
        }
        Ok(())
    }

    pub async fn list_watchers(pool: &str, name: &str) -> Result<Vec<String>> {
        let io = Self::connect(pool)?;
        let name_c = CString::new(name)?;
        let mut result = Vec::new();

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            if rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null()) < 0 {
                return Ok(vec![]);
            }

            let mut watchers: *mut rbd_image_watcher_t = ptr::null_mut();
            let mut num_watchers: libc::size_t = 0;

            if rbd_watchers_list(image, &mut watchers, &mut num_watchers) >= 0
                && !watchers.is_null()
            {
                let watcher_slice = std::slice::from_raw_parts(watchers, num_watchers);
                for w in watcher_slice {
                    let addr = std::ffi::CStr::from_ptr(w.addr.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                    result.push(addr);
                }
                rbd_watchers_list_cleanup(watchers, num_watchers);
            }

            rbd_close(image);
        }
        Ok(result)
    }

    pub async fn delete_volume(pool: &str, name: &str) -> Result<()> {
        info!("Deleting RBD volume: {}/{}", pool, name);

        // Check for active watchers first
        let watchers = Self::list_watchers(pool, name).await?;
        if !watchers.is_empty() {
            return Err(anyhow!(
                "Cannot delete volume: it is still in use by active watchers. {}",
                watchers.join(", ")
            ));
        }

        // Try to unmap first in case it's still mapped
        let spec = format!("{pool}/{name}");
        if let Err(e) = Self::unmap_volume(&spec).await {
            warn!("Failed to unmap volume before deletion {}: {}", spec, e);
        }

        // Purge snapshots before removing (Ceph requires this)
        if let Err(e) = Self::purge_image_snapshots(pool, name).await {
            warn!("Failed to purge snapshots before deletion {}/{}: {}", pool, name, e);
        }

        let io = Self::connect(pool)?;
        let name_c = CString::new(name).map_err(|e| anyhow!("Invalid volume name: {}", e))?;

        unsafe {
            let ret = rbd_remove(io.handle, name_c.as_ptr());
            if ret < 0 && ret != -2 {
                return Err(anyhow!("rbd_remove failed with code {}", ret));
            }
        }
        Ok(())
    }

    pub async fn create_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        info!(
            "Creating RBD snapshot (native FFI): {}/{}@{}",
            pool, name, snapshot_name
        );
        let io = Self::connect(pool)?;
        let name_c = CString::new(name).map_err(|e| anyhow!("Invalid volume name: {}", e))?;
        let snap_c =
            CString::new(snapshot_name).map_err(|e| anyhow!("Invalid snapshot name: {}", e))?;

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            if rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null()) < 0 {
                return Err(anyhow!("Failed to open image {} for snapshot", name));
            }

            let ret = rbd_snap_create(image, snap_c.as_ptr());
            rbd_close(image);

            if ret < 0 {
                return Err(anyhow!(
                    "rbd_snap_create failed for {}@{} with code {}",
                    name,
                    snapshot_name,
                    ret
                ));
            }
        }
        Ok(())
    }

    pub async fn delete_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        info!(
            "Deleting RBD snapshot (native FFI): {}/{}@{}",
            pool, name, snapshot_name
        );
        let io = Self::connect(pool)?;
        let name_c = CString::new(name).map_err(|e| anyhow!("Invalid volume name: {}", e))?;
        let snap_c =
            CString::new(snapshot_name).map_err(|e| anyhow!("Invalid snapshot name: {}", e))?;

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            if rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null()) < 0 {
                return Err(anyhow!(
                    "Failed to open image {} for snapshot deletion",
                    name
                ));
            }

            let mut ret = librbd_sys::rbd_snap_remove(image, snap_c.as_ptr());

            // If it failed because it's protected, try to unprotect and retry
            if ret == -16 {
                info!(
                    "Snapshot {}@{} is protected, attempting to unprotect...",
                    name, snapshot_name
                );
                let unprot_ret = rbd_snap_unprotect(image, snap_c.as_ptr());
                if unprot_ret == 0 {
                    ret = librbd_sys::rbd_snap_remove(image, snap_c.as_ptr());
                } else if unprot_ret == -16 {
                    warn!(
                        "Failed to unprotect snapshot {}@{}: it still has child clones",
                        name, snapshot_name
                    );
                }
            }

            rbd_close(image);

            if ret < 0 {
                return Err(anyhow!(
                    "rbd_snap_remove failed for {}@{} with code {}",
                    name,
                    snapshot_name,
                    ret
                ));
            }
        }
        Ok(())
    }

    pub async fn restore_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        info!(
            "Restoring RBD snapshot (native FFI): {}/{}@{}",
            pool, name, snapshot_name
        );
        let io = Self::connect(pool)?;
        let name_c = CString::new(name).map_err(|e| anyhow!("Invalid volume name: {}", e))?;
        let snap_c =
            CString::new(snapshot_name).map_err(|e| anyhow!("Invalid snapshot name: {}", e))?;

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            let ret_open = rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null());
            if ret_open < 0 {
                if ret_open == -16 {
                    return Err(anyhow!(
                        "Failed to open image {}: Image is busy/locked by another client. Stop the VM using this volume first.",
                        name
                    ));
                }
                return Err(anyhow!(
                    "Failed to open image {} for snapshot restoration (code {})",
                    name,
                    ret_open
                ));
            }

            let ret = rbd_snap_rollback(image, snap_c.as_ptr());
            rbd_close(image);

            if ret < 0 {
                if ret == -16 {
                    return Err(anyhow!(
                        "Failed to restore snapshot {}@{}: Image is busy (likely mapped to a VM). Stop the VM first.",
                        name,
                        snapshot_name
                    ));
                }
                return Err(anyhow!(
                    "rbd_snap_rollback failed for {}@{} with code {}",
                    name,
                    snapshot_name,
                    ret
                ));
            }
        }
        Ok(())
    }

    pub async fn clone_volume(
        pool: &str,
        source_name: &str,
        snapshot_name: &str,
        target_name: &str,
    ) -> Result<()> {
        info!(
            "Cloning RBD volume (native FFI): {}/{}@{} to {}",
            pool, source_name, snapshot_name, target_name
        );
        let io = Self::connect(pool)?;
        let source_c =
            CString::new(source_name).map_err(|e| anyhow!("Invalid source volume name: {}", e))?;
        let snap_c =
            CString::new(snapshot_name).map_err(|e| anyhow!("Invalid snapshot name: {}", e))?;
        let target_c =
            CString::new(target_name).map_err(|e| anyhow!("Invalid target volume name: {}", e))?;

        unsafe {
            let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
            let ret_open = rbd_open(io.handle, source_c.as_ptr(), &mut image, ptr::null());
            if ret_open < 0 {
                if ret_open == -16 {
                    return Err(anyhow!(
                        "Failed to open source image {}: Image is busy/locked. Stop the VM using this volume first to allow cloning.",
                        source_name
                    ));
                }
                return Err(anyhow!(
                    "Failed to open source image {} for cloning (code {})",
                    source_name,
                    ret_open
                ));
            }

            // Attempt to protect the snapshot (ignore error if already protected)
            let prot_ret = rbd_snap_protect(image, snap_c.as_ptr());
            if prot_ret < 0 && prot_ret != -16 {
                // -16 is EEXIST/EBUSY (already protected)
                rbd_close(image);
                return Err(anyhow!(
                    "Failed to protect snapshot {}@{}: code {}",
                    source_name,
                    snapshot_name,
                    prot_ret
                ));
            }

            let mut order = 0;
            // RBD_FEATURE_LAYERING = 1
            let ret = rbd_clone(
                io.handle,
                source_c.as_ptr(),
                snap_c.as_ptr(),
                io.handle,
                target_c.as_ptr(),
                1,
                &mut order,
            );

            rbd_close(image);

            if ret < 0 {
                return Err(anyhow!(
                    "rbd_clone failed for {}@{} to {} with code {}",
                    source_name,
                    snapshot_name,
                    target_name,
                    ret
                ));
            }
        }
        Ok(())
    }

    pub async fn exists(pool: &str, name: &str) -> bool {
        if let Ok(io) = Self::connect(pool)
            && let Ok(name_c) = CString::new(name)
        {
            unsafe {
                let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
                let ret = rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null());
                if ret == 0 {
                    rbd_close(image);
                    return true;
                }
            }
        }
        false
    }

    pub async fn map_volume(pool: &str, name: &str) -> Result<String> {
        info!("Mapping RBD image natively: {}/{}", pool, name);

        // 1. Get monitors from config
        let mon_host = {
            let io = Self::connect(pool)?;
            let mut mon_host = vec![0u8; 1024];
            unsafe {
                let ret = librados_sys::rados_conf_get(
                    io._cluster.0,
                    CString::new("mon_host")?.as_ptr(),
                    mon_host.as_mut_ptr() as *mut libc::c_char,
                    mon_host.len(),
                );
                if ret < 0 {
                    return Err(anyhow!("Failed to get mon_host from ceph config"));
                }
            }
            let host = std::ffi::CStr::from_bytes_until_nul(&mon_host)?
                .to_str()?
                .to_string();

            // 2. Disable features that the kernel might not support (same as before but via FFI)
            unsafe {
                let mut image: librbd_sys::rbd_image_t = ptr::null_mut();
                let name_c = CString::new(name)?;
                if rbd_open(io.handle, name_c.as_ptr(), &mut image, ptr::null()) == 0 {
                    // RBD_FEATURE_EXCLUSIVE_LOCK = 4, RBD_FEATURE_OBJECT_MAP = 8, RBD_FEATURE_FAST_DIFF = 16, RBD_FEATURE_DEEP_FLATTEN = 32
                    let features_to_disable = 4 | 8 | 16 | 32;
                    rbd_update_features(image, features_to_disable, 0);
                    rbd_close(image);
                }
            }
            host
        }; // io is dropped here

        // 3. Write to /sys/bus/rbd/add
        // ... rest of code
        // Format: <mon_addrs> name=<admin_name>,secret=<admin_key> <pool_name> <image_name> [snap_name]
        // Since we are using client.admin with a keyring, we might need to find the key.
        // For simplicity and matching common setups, we'll try to use the admin key from /etc/ceph/admin.secret if it exists,
        // or just use the system's rbd kernel module defaults.

        let secret = match tokio::fs::read_to_string("/etc/ceph/admin.secret").await {
            Ok(s) => s.trim().to_string(),
            Err(_) => {
                return Err(anyhow!(
                    "Missing /etc/ceph/admin.secret for native RBD mapping"
                ));
            },
        };

        // Validate identifiers before passing them to the kernel.
        if pool.contains(|c: char| c.is_whitespace() || c == '\0' || c == ',') {
            return Err(anyhow!(
                "Invalid RBD pool name: contains forbidden characters"
            ));
        }
        if name.contains(|c: char| c.is_whitespace() || c == '\0' || c == ',') {
            return Err(anyhow!(
                "Invalid RBD image name: contains forbidden characters"
            ));
        }

        let map_cmd = format!(
            "{} name=admin,secret={} {} {}",
            mon_host, secret, pool, name
        );
        tokio::fs::write("/sys/bus/rbd/add", map_cmd)
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to write to /sys/bus/rbd/add: {}. Ensure rbd kernel module is loaded.",
                    e
                )
            })?;

        // 4. Find the mapped device
        // We look into /sys/bus/rbd/devices/ to find the one matching our image and pool
        let mut entries = tokio::fs::read_dir("/sys/bus/rbd/devices").await?;
        while let Some(entry) = entries.next_entry().await? {
            let id = entry.file_name();
            let dev_pool = tokio::fs::read_to_string(format!(
                "/sys/bus/rbd/devices/{}/pool",
                id.to_string_lossy()
            ))
            .await?;
            let dev_name = tokio::fs::read_to_string(format!(
                "/sys/bus/rbd/devices/{}/name",
                id.to_string_lossy()
            ))
            .await?;

            if dev_pool.trim() == pool && dev_name.trim() == name {
                let dev_path = format!("/dev/rbd{}", id.to_string_lossy());
                info!("RBD image mapped natively to {}", dev_path);
                return Ok(dev_path);
            }
        }

        Err(anyhow!(
            "Failed to find mapped RBD device after writing to /sys/bus/rbd/add"
        ))
    }

    pub async fn unmap_volume(device_or_spec: &str) -> Result<()> {
        info!("Unmapping RBD device/spec natively: {}", device_or_spec);

        let id = if let Some(stripped) = device_or_spec.strip_prefix("/dev/rbd") {
            stripped.to_string()
        } else {
            // It's a spec like pool/name, we need to find the ID
            let mut found_id = None;
            let mut entries = tokio::fs::read_dir("/sys/bus/rbd/devices").await?;
            while let Some(entry) = entries.next_entry().await? {
                let id_str = entry.file_name().to_string_lossy().into_owned();
                let dev_pool =
                    tokio::fs::read_to_string(format!("/sys/bus/rbd/devices/{}/pool", id_str))
                        .await?;
                let dev_name =
                    tokio::fs::read_to_string(format!("/sys/bus/rbd/devices/{}/name", id_str))
                        .await?;
                let spec = format!("{}/{}", dev_pool.trim(), dev_name.trim());
                if spec == device_or_spec {
                    found_id = Some(id_str);
                    break;
                }
            }
            match found_id {
                Some(id) => id,
                None => return Ok(()), // Not mapped
            }
        };

        if let Err(e) = tokio::fs::write("/sys/bus/rbd/remove", &id).await {
            warn!("Failed to unmap RBD device {}: {}", id, e);
        }
        Ok(())
    }
}

impl StorageProvider for CephRbd {
    async fn ensure_pool_exists(&self, pool: &str) -> Result<()> {
        CephRbd::ensure_pool_exists(pool).await
    }

    async fn create_volume(&self, pool: &str, name: &str, size_mib: i32) -> Result<()> {
        CephRbd::create_volume(pool, name, size_mib).await
    }

    async fn delete_volume(&self, pool: &str, name: &str) -> Result<()> {
        CephRbd::delete_volume(pool, name).await
    }

    async fn create_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        CephRbd::create_snapshot(pool, name, snapshot_name).await
    }

    async fn delete_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        CephRbd::delete_snapshot(pool, name, snapshot_name).await
    }

    async fn restore_snapshot(&self, pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
        CephRbd::restore_snapshot(pool, name, snapshot_name).await
    }

    async fn clone_volume(
        &self,
        pool: &str,
        source_name: &str,
        snapshot_name: &str,
        target_name: &str,
    ) -> Result<()> {
        CephRbd::clone_volume(pool, source_name, snapshot_name, target_name).await
    }

    async fn exists(&self, pool: &str, name: &str) -> bool {
        CephRbd::exists(pool, name).await
    }

    async fn map_volume(&self, pool: &str, name: &str) -> Result<String> {
        CephRbd::map_volume(pool, name).await
    }

    async fn unmap_volume(&self, device_path: &str) -> Result<()> {
        CephRbd::unmap_volume(device_path).await
    }
}

pub struct CephFs;

impl CephFs {
    pub async fn mount_volume(volume_id: &str, mount_point: &str) -> Result<()> {
        info!(
            "Mounting CephFS volume natively: {} to {}",
            volume_id, mount_point
        );

        // Ensure mount point exists
        tokio::fs::create_dir_all(mount_point).await?;

        // 1. Get monitors from config
        let mon_host = {
            let cluster_id = CString::new("admin")?;
            let mut cluster: rados_t = ptr::null_mut();
            unsafe {
                if rados_create(&mut cluster, cluster_id.as_ptr()) < 0 {
                    return Err(anyhow!("Failed to create rados handle"));
                }
                let conf_ret = rados_conf_read_file(
                    cluster,
                    CString::new("/etc/ceph/ceph.conf")?.as_ptr(),
                );
                if conf_ret < 0 {
                    warn!("Failed to read ceph.conf in CephFs::mount_volume: {}", conf_ret);
                }
                let mut mon_host = vec![0u8; 1024];
                librados_sys::rados_conf_get(
                    cluster,
                    CString::new("mon_host")?.as_ptr(),
                    mon_host.as_mut_ptr() as *mut libc::c_char,
                    mon_host.len(),
                );
                let host = std::ffi::CStr::from_bytes_until_nul(&mon_host)?
                    .to_str()?
                    .to_string();
                rados_shutdown(cluster);
                host
            }
        };

        // 2. Perform mount
        if volume_id.contains(|c: char| c.is_whitespace() || c == '\0' || c == '/') {
            return Err(anyhow!(
                "Invalid CephFS volume_id: contains forbidden characters"
            ));
        }
        let source = format!("{}:/volumes/{}", mon_host, volume_id);
        let source_c = CString::new(source)?;
        let target_c = CString::new(mount_point)?;
        let fstype_c = CString::new("ceph")?;
        let options_c = CString::new("name=admin,secretfile=/etc/ceph/admin.secret")?;

        let volume_id_owned = volume_id.to_string();

        tokio::task::spawn_blocking(move || {
            let ret = unsafe {
                libc::mount(
                    source_c.as_ptr(),
                    target_c.as_ptr(),
                    fstype_c.as_ptr(),
                    0,
                    options_c.as_ptr() as *const libc::c_void,
                )
            };

            if ret != 0 {
                let err = std::io::Error::last_os_error();
                Err(anyhow!(
                    "Failed to mount CephFS volume {}: {}",
                    volume_id_owned,
                    err
                ))
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|e| anyhow!("Blocking task failed: {}", e))?
    }

    pub async fn unmount_volume(mount_point: &str) -> Result<()> {
        info!("Unmounting CephFS volume natively from {}", mount_point);
        let mount_point_c = CString::new(mount_point)?;
        let mount_point_owned = mount_point.to_string();
        tokio::task::spawn_blocking(move || {
            let ret = unsafe { libc::umount2(mount_point_c.as_ptr(), libc::MNT_DETACH) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                warn!("Failed to unmount natively {}: {}", mount_point_owned, err);
            }
        })
        .await
        .map_err(|e| anyhow!("Blocking task failed: {}", e))
    }
}
