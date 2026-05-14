use anyhow::{Result, anyhow};
use librados_sys::{
    rados_conf_read_file, rados_connect, rados_create, rados_ioctx_create, rados_ioctx_destroy,
    rados_ioctx_t, rados_pool_create, rados_shutdown, rados_t,
};
use librbd_sys::{rbd_close, rbd_create, rbd_open, rbd_remove, rbd_snap_create};
use std::ffi::CString;
use std::process::Command;
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
}

pub struct CephRbd;

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
    pub fn ensure_pool_exists(pool: &str) -> Result<()> {
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
            let _ = rados_conf_read_file(cluster.0, config.as_ptr());
            let _ = rados_connect(cluster.0);

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

    pub fn create_volume(pool: &str, name: &str, size_mib: i32) -> Result<()> {
        Self::ensure_pool_exists(pool)?;

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

    pub fn delete_volume(pool: &str, name: &str) -> Result<()> {
        info!("Deleting RBD volume (native FFI): {}/{}", pool, name);
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

    pub fn create_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
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

    pub fn delete_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
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

    pub fn restore_snapshot(pool: &str, name: &str, snapshot_name: &str) -> Result<()> {
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

    pub fn clone_volume(
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

    pub fn exists(pool: &str, name: &str) -> bool {
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

    pub fn map_volume(pool: &str, name: &str) -> Result<String> {
        info!("Mapping RBD image to kernel: {}/{}", pool, name);
        let output = Command::new("rbd")
            .arg("map")
            .arg(format!("{}/{}", pool, name))
            .output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("rbd map failed: {}", err));
        }

        let device_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!("RBD image mapped to {}", device_path);
        Ok(device_path)
    }

    pub fn unmap_volume(device_path: &str) -> Result<()> {
        info!("Unmapping RBD device: {}", device_path);
        let status = Command::new("rbd").arg("unmap").arg(device_path).status()?;

        if !status.success() {
            return Err(anyhow!("rbd unmap failed for {}", device_path));
        }
        Ok(())
    }
}
