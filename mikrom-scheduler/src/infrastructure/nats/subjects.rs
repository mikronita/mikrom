//! Local NATS subject aliases used by the scheduler.
//!
//! Shared subjects live in `mikrom_proto::subjects`. These aliases cover
//! scheduler-specific subjects that are not part of the shared proto contract
//! yet, and let the event loop avoid hard-coded string literals.

pub const WORKER_HEARTBEAT: &str = "mikrom.scheduler.worker.heartbeat";
pub const ROUTER_HEARTBEAT: &str = "mikrom.scheduler.router.heartbeat";
pub const UPDATE_SECURITY_GROUPS: &str = "mikrom.scheduler.update_security_groups";
pub const CHECK_HEALTH: &str = "mikrom.scheduler.check_health";
pub const DELETE_ALL_BY_APP: &str = "mikrom.scheduler.delete_all_by_app";
pub const CREATE_VOLUME: &str = "mikrom.scheduler.create_volume";
pub const CREATE_SNAPSHOT: &str = "mikrom.scheduler.create_snapshot";
pub const DELETE_VOLUME: &str = "mikrom.scheduler.delete_volume";
pub const DELETE_SNAPSHOT: &str = "mikrom.scheduler.delete_snapshot";
pub const RESTORE_SNAPSHOT: &str = "mikrom.scheduler.restore_snapshot";
pub const CLONE_VOLUME: &str = "mikrom.scheduler.clone_volume";
pub const VM_SNAPSHOT_CREATE: &str = "mikrom.scheduler.vm_snapshot_create";
pub const VM_SNAPSHOT_RESTORE: &str = "mikrom.scheduler.vm_snapshot_restore";
pub const VM_SNAPSHOT_DELETE: &str = "mikrom.scheduler.vm_snapshot_delete";
pub const VM_SNAPSHOT_LIST: &str = "mikrom.scheduler.vm_snapshot_list";
pub const ATTACH_VOLUME: &str = "mikrom.scheduler.attach_volume";
pub const DETACH_VOLUME: &str = "mikrom.scheduler.detach_volume";
pub const START_MIGRATION: &str = "mikrom.scheduler.start_migration";
pub const CANCEL_MIGRATION: &str = "mikrom.scheduler.cancel_migration";
pub const QUERY_MIGRATION: &str = "mikrom.scheduler.query_migration";
pub const SET_BALLOON: &str = "mikrom.scheduler.set_balloon";
pub const QUERY_BALLOON: &str = "mikrom.scheduler.query_balloon";
