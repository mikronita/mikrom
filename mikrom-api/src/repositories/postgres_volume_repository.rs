use crate::models::volume::{Volume, VolumeSnapshot};
use crate::repositories::volume_repository::{
    CreateSnapshotParams, CreateVolumeParams, VolumeRepository,
};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresVolumeRepository {
    pool: PgPool,
}

impl PostgresVolumeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl VolumeRepository for PostgresVolumeRepository {
    async fn create_volume(&self, params: CreateVolumeParams) -> anyhow::Result<Volume> {
        let volume = sqlx::query_as::<_, Volume>(
            "INSERT INTO volumes (app_id, user_id, name, size_mib, pool_name, mount_point) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"
        )
        .bind(params.app_id)
        .bind(params.user_id)
        .bind(params.name)
        .bind(params.size_mib)
        .bind(params.pool_name)
        .bind(params.mount_point)
        .fetch_one(&self.pool)
        .await?;

        Ok(volume)
    }

    async fn get_volume(&self, volume_id: Uuid) -> anyhow::Result<Option<Volume>> {
        let volume = sqlx::query_as::<_, Volume>("SELECT * FROM volumes WHERE id = $1")
            .bind(volume_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(volume)
    }

    async fn list_volumes_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<Volume>> {
        let volumes = sqlx::query_as::<_, Volume>("SELECT * FROM volumes WHERE app_id = $1")
            .bind(app_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(volumes)
    }

    async fn delete_volume(&self, volume_id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM volumes WHERE id = $1")
            .bind(volume_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn create_snapshot(
        &self,
        params: CreateSnapshotParams,
    ) -> anyhow::Result<VolumeSnapshot> {
        let snapshot = sqlx::query_as::<_, VolumeSnapshot>(
            "INSERT INTO volume_snapshots (volume_id, user_id, name) VALUES ($1, $2, $3) RETURNING *"
        )
        .bind(params.volume_id)
        .bind(params.user_id)
        .bind(params.name)
        .fetch_one(&self.pool)
        .await?;

        Ok(snapshot)
    }

    async fn get_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<Option<VolumeSnapshot>> {
        let snapshot =
            sqlx::query_as::<_, VolumeSnapshot>("SELECT * FROM volume_snapshots WHERE id = $1")
                .bind(snapshot_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(snapshot)
    }

    async fn list_snapshots_by_volume(
        &self,
        volume_id: Uuid,
    ) -> anyhow::Result<Vec<VolumeSnapshot>> {
        let snapshots = sqlx::query_as::<_, VolumeSnapshot>(
            "SELECT * FROM volume_snapshots WHERE volume_id = $1",
        )
        .bind(volume_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(snapshots)
    }

    async fn delete_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM volume_snapshots WHERE id = $1")
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
