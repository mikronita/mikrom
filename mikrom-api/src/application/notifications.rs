use crate::AppState;
use crate::infrastructure::db::models::DbWorkspaceNotification;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub route: String,
    pub entity_name: Option<String>,
    pub resource_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub is_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct NotificationListResponse {
    pub notifications: Vec<Notification>,
    pub unread_count: i64,
    pub has_more: bool,
    pub next_offset: i64,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct NotificationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub unread_only: Option<bool>,
}

impl From<DbWorkspaceNotification> for Notification {
    fn from(row: DbWorkspaceNotification) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            tenant_id: row.tenant_id,
            kind: row.kind,
            title: row.title,
            body: row.body,
            route: row.route,
            entity_name: row.entity_name,
            resource_id: row.resource_id,
            metadata: row.metadata,
            created_at: row.created_at,
            read_at: row.read_at,
            is_read: row.read_at.is_some(),
        }
    }
}

fn truncate_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(20).clamp(1, 100)
}

fn truncate_offset(offset: Option<i64>) -> i64 {
    offset.unwrap_or(0).max(0)
}

fn event_kind_name(kind: &WorkspaceEventKind) -> &'static str {
    match kind {
        WorkspaceEventKind::AppCreated => "app_created",
        WorkspaceEventKind::AppUpdated => "app_updated",
        WorkspaceEventKind::AppDeleted => "app_deleted",
        WorkspaceEventKind::DeploymentChanged => "deployment_changed",
        WorkspaceEventKind::ProfileUpdated => "profile_updated",
        WorkspaceEventKind::GithubAccountsChanged => "github_accounts_changed",
        WorkspaceEventKind::BillingUpdated => "billing_updated",
        WorkspaceEventKind::SecurityRulesChanged => "security_rules_changed",
        WorkspaceEventKind::VolumeChanged => "volume_changed",
        WorkspaceEventKind::SnapshotChanged => "snapshot_changed",
        WorkspaceEventKind::DatabaseCreated => "database_created",
        WorkspaceEventKind::DatabaseUpdated => "database_updated",
        WorkspaceEventKind::DatabaseDeleted => "database_deleted",
        WorkspaceEventKind::Refresh => "refresh",
    }
}

fn notification_payload(event: &WorkspaceEvent) -> Option<(String, String, String, String)> {
    let entity_name = event.app_name.clone().or_else(|| event.resource_id.clone());

    match event.kind {
        WorkspaceEventKind::AppCreated => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "application".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Application created".to_string(),
                format!("Application {name} was created."),
                if let Some(app_name) = event.app_name.as_deref() {
                    format!("/apps/{app_name}")
                } else {
                    "/apps".to_string()
                },
            ))
        },
        WorkspaceEventKind::AppUpdated => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "application".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Application updated".to_string(),
                format!("Application {name} was updated."),
                if let Some(app_name) = event.app_name.as_deref() {
                    format!("/apps/{app_name}")
                } else {
                    "/apps".to_string()
                },
            ))
        },
        WorkspaceEventKind::AppDeleted => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "application".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Application deleted".to_string(),
                format!("Application {name} was deleted."),
                "/apps".to_string(),
            ))
        },
        WorkspaceEventKind::DeploymentChanged => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "deployment".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Deployment changed".to_string(),
                format!("Deployment activity was recorded for {name}."),
                if let Some(app_name) = event.app_name.as_deref() {
                    format!("/apps/{app_name}")
                } else {
                    "/apps".to_string()
                },
            ))
        },
        WorkspaceEventKind::ProfileUpdated => Some((
            event_kind_name(&event.kind).to_string(),
            "Profile updated".to_string(),
            "Your profile was updated.".to_string(),
            "/settings".to_string(),
        )),
        WorkspaceEventKind::GithubAccountsChanged => Some((
            event_kind_name(&event.kind).to_string(),
            "GitHub connected".to_string(),
            "Your GitHub integrations changed.".to_string(),
            "/settings".to_string(),
        )),
        WorkspaceEventKind::BillingUpdated => Some((
            event_kind_name(&event.kind).to_string(),
            "Billing updated".to_string(),
            "Your billing status changed.".to_string(),
            "/settings".to_string(),
        )),
        WorkspaceEventKind::SecurityRulesChanged => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "application".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Security rules changed".to_string(),
                format!("Security rules were updated for {name}."),
                if let Some(app_name) = event.app_name.as_deref() {
                    format!("/apps/{app_name}")
                } else {
                    "/apps".to_string()
                },
            ))
        },
        WorkspaceEventKind::VolumeChanged => Some((
            event_kind_name(&event.kind).to_string(),
            "Storage updated".to_string(),
            "A storage volume changed.".to_string(),
            "/storage".to_string(),
        )),
        WorkspaceEventKind::SnapshotChanged => Some((
            event_kind_name(&event.kind).to_string(),
            "Snapshot updated".to_string(),
            "A snapshot changed.".to_string(),
            "/storage".to_string(),
        )),
        WorkspaceEventKind::DatabaseCreated => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "database".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Database created".to_string(),
                format!("Database {name} was created."),
                if let Some(name) = event.resource_id.as_deref() {
                    format!("/databases/{name}")
                } else {
                    "/databases".to_string()
                },
            ))
        },
        WorkspaceEventKind::DatabaseUpdated => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "database".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Database updated".to_string(),
                format!("Database {name} was updated."),
                if let Some(name) = event.resource_id.as_deref() {
                    format!("/databases/{name}")
                } else {
                    "/databases".to_string()
                },
            ))
        },
        WorkspaceEventKind::DatabaseDeleted => {
            let name = entity_name
                .clone()
                .unwrap_or_else(|| "database".to_string());
            Some((
                event_kind_name(&event.kind).to_string(),
                "Database deleted".to_string(),
                format!("Database {name} was deleted."),
                "/databases".to_string(),
            ))
        },
        WorkspaceEventKind::Refresh => None,
    }
}

async fn recipient_user_ids(
    pool: &sqlx::PgPool,
    event: &WorkspaceEvent,
) -> anyhow::Result<Vec<Uuid>> {
    if let Some(user_id) = event.user_id {
        return Ok(vec![user_id]);
    }

    let Some(tenant_id) = event.tenant_id else {
        return Ok(Vec::new());
    };

    let user_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM tenant_members WHERE tenant_id = $1 ORDER BY user_id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;

    Ok(user_ids)
}

pub async fn project_workspace_event(
    state: &AppState,
    event: WorkspaceEvent,
) -> anyhow::Result<()> {
    let Some((kind, title, body, route)) = notification_payload(&event) else {
        return Ok(());
    };

    let recipients = recipient_user_ids(&state.api_db, &event).await?;
    if recipients.is_empty() {
        return Ok(());
    }

    let metadata = serde_json::to_value(&event)?;

    for user_id in recipients {
        let notification_id = Uuid::new_v4();
        let entity_name = event.app_name.clone().or_else(|| event.resource_id.clone());

        sqlx::query(
            "INSERT INTO workspace_notifications (id, user_id, tenant_id, kind, title, body, route, entity_name, resource_id, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(notification_id)
        .bind(user_id)
        .bind(event.tenant_id)
        .bind(&kind)
        .bind(&title)
        .bind(&body)
        .bind(&route)
        .bind(&entity_name)
        .bind(&event.resource_id)
        .bind(&metadata)
        .execute(&state.api_db)
        .await?;
    }

    Ok(())
}

pub async fn list_notifications(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    limit: Option<i64>,
    offset: Option<i64>,
    unread_only: bool,
) -> anyhow::Result<NotificationListResponse> {
    let limit = truncate_limit(limit);
    let offset = truncate_offset(offset);

    let query = if unread_only {
        sqlx::query_as::<_, DbWorkspaceNotification>(
            "SELECT id, user_id, tenant_id, kind, title, body, route, entity_name, resource_id, metadata, created_at, read_at FROM workspace_notifications WHERE user_id = $1 AND read_at IS NULL ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
    } else {
        sqlx::query_as::<_, DbWorkspaceNotification>(
            "SELECT id, user_id, tenant_id, kind, title, body, route, entity_name, resource_id, metadata, created_at, read_at FROM workspace_notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
    };

    let mut notifications = query
        .bind(user_id)
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    let has_more = notifications.len() as i64 > limit;
    if has_more {
        notifications.pop();
    }
    let visible_count = notifications.len() as i64;
    let next_offset = offset + visible_count;

    let unread_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workspace_notifications WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(NotificationListResponse {
        notifications: notifications.into_iter().map(Into::into).collect(),
        unread_count,
        has_more,
        next_offset,
    })
}

pub async fn mark_notification_read(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    notification_id: Uuid,
) -> anyhow::Result<bool> {
    let updated = sqlx::query(
        "UPDATE workspace_notifications SET read_at = NOW() WHERE id = $1 AND user_id = $2 AND read_at IS NULL",
    )
    .bind(notification_id)
    .bind(user_id)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(updated > 0)
}

pub async fn mark_all_notifications_read(
    pool: &sqlx::PgPool,
    user_id: Uuid,
) -> anyhow::Result<u64> {
    let updated = sqlx::query(
        "UPDATE workspace_notifications SET read_at = NOW() WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(updated)
}
