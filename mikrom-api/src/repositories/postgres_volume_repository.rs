use crate::models::volume::{
    AppVolume, AttachedVolume, Volume, VolumeAttachmentInfo, VolumeSnapshot, VolumeWithAttachments,
};
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
            "INSERT INTO volumes (user_id, name, size_mib, pool_name) VALUES ($1, $2, $3, $4) RETURNING *"
        )
        .bind(params.user_id)
        .bind(params.name)
        .bind(params.size_mib)
        .bind(params.pool_name)
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

    async fn list_volumes_by_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<VolumeWithAttachments>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                v.id as volume_id, v.user_id, v.name as volume_name, v.size_mib, v.pool_name, v.created_at as vol_created_at, v.updated_at as vol_updated_at,
                av.app_id as "app_id?", av.mount_point as "mount_point?", av.access_mode as "access_mode?",
                a.name as "app_name?"
            FROM volumes v
            LEFT JOIN app_volumes av ON v.id = av.volume_id
            LEFT JOIN apps a ON av.app_id = a.id
            WHERE v.user_id = $1
            ORDER BY v.created_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut volumes_map: std::collections::HashMap<Uuid, VolumeWithAttachments> =
            std::collections::HashMap::new();
        let mut ordered_ids = Vec::new();

        for row in rows {
            let entry = volumes_map.entry(row.volume_id).or_insert_with(|| {
                ordered_ids.push(row.volume_id);
                VolumeWithAttachments {
                    volume: Volume {
                        id: row.volume_id,
                        user_id: row.user_id,
                        name: row.volume_name.clone(),
                        size_mib: row.size_mib,
                        pool_name: row.pool_name.clone(),
                        created_at: row.vol_created_at,
                        updated_at: row.vol_updated_at,
                    },
                    attachments: Vec::new(),
                }
            });

            if let (Some(app_id), Some(app_name), Some(mount_point), Some(access_mode)) =
                (row.app_id, row.app_name, row.mount_point, row.access_mode)
            {
                entry.attachments.push(VolumeAttachmentInfo {
                    app_id,
                    app_name,
                    mount_point,
                    access_mode,
                });
            }
        }

        let result = ordered_ids
            .into_iter()
            .filter_map(|id| volumes_map.remove(&id))
            .collect();

        Ok(result)
    }

    async fn is_volume_attached(&self, volume_id: Uuid) -> anyhow::Result<bool> {
        let row = sqlx::query!(
            "SELECT count(*) FROM app_volumes WHERE volume_id = $1",
            volume_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count.unwrap_or(0) > 0)
    }

    async fn delete_volume(&self, volume_id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM volumes WHERE id = $1")
            .bind(volume_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn attach_volume_to_app(
        &self,
        app_id: Uuid,
        volume_id: Uuid,
        mount_point: String,
        access_mode: i32,
    ) -> anyhow::Result<AppVolume> {
        let app_volume = sqlx::query_as::<_, AppVolume>(
            "INSERT INTO app_volumes (app_id, volume_id, mount_point, access_mode) VALUES ($1, $2, $3, $4) 
             ON CONFLICT (app_id, volume_id) DO UPDATE SET mount_point = EXCLUDED.mount_point, access_mode = EXCLUDED.access_mode
             RETURNING *"
        )
        .bind(app_id)
        .bind(volume_id)
        .bind(mount_point)
        .bind(access_mode)
        .fetch_one(&self.pool)
        .await?;

        Ok(app_volume)
    }

    async fn detach_volume_from_app(&self, app_id: Uuid, volume_id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM app_volumes WHERE app_id = $1 AND volume_id = $2")
            .bind(app_id)
            .bind(volume_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_volumes_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<AttachedVolume>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                v.id, v.user_id, v.name, v.size_mib, v.pool_name, v.created_at, v.updated_at,
                av.mount_point, av.access_mode
            FROM volumes v
            JOIN app_volumes av ON v.id = av.volume_id
            WHERE av.app_id = $1
            "#,
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await?;

        let volumes = rows
            .into_iter()
            .map(|row| {
                use sqlx::Row;
                AttachedVolume {
                    volume: Volume {
                        id: row.get("id"),
                        user_id: row.get("user_id"),
                        name: row.get("name"),
                        size_mib: row.get("size_mib"),
                        pool_name: row.get("pool_name"),
                        created_at: row.get("created_at"),
                        updated_at: row.get("updated_at"),
                    },
                    mount_point: row.get("mount_point"),
                    access_mode: row.get("access_mode"),
                }
            })
            .collect();

        Ok(volumes)
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
