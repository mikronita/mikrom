use crate::models::app::{App, Deployment};
use crate::repositories::app_repository::{AppRepository, NewDeployment};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresAppRepository {
    pool: PgPool,
}

impl PostgresAppRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AppRepository for PostgresAppRepository {
    async fn create_app(
        &self,
        name: &str,
        git_url: &str,
        port: i32,
        hostname: Option<String>,
        user_id: &str,
        github_webhook_secret: Option<String>,
    ) -> anyhow::Result<App> {
        let uid = Uuid::parse_str(user_id)?;
        let result = sqlx::query_as::<_, App>(
            "INSERT INTO apps (name, git_url, port, hostname, user_id, github_webhook_secret) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"
        )
        .bind(name)
        .bind(git_url)
        .bind(port)
        .bind(hostname)
        .bind(uid)
        .bind(github_webhook_secret)
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(app) => Ok(app),
            Err(e) => {
                if let Some(db_err) = e.as_database_error()
                    && db_err.code().as_deref() == Some("23505")
                {
                    return Err(anyhow::anyhow!(
                        "Application name '{}' is already taken",
                        name
                    ));
                }
                Err(e.into())
            },
        }
    }

    async fn get_app(&self, id: Uuid) -> anyhow::Result<Option<App>> {
        let app = sqlx::query_as::<_, App>("SELECT * FROM apps WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(app)
    }

    async fn get_app_by_name(&self, name: &str) -> anyhow::Result<Option<App>> {
        let app = sqlx::query_as::<_, App>("SELECT * FROM apps WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        Ok(app)
    }

    async fn delete_app(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM apps WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn list_apps_by_user(&self, user_id: &str) -> anyhow::Result<Vec<App>> {
        let apps = if user_id == "all" {
            sqlx::query_as::<_, App>("SELECT * FROM apps ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?
        } else {
            let uid = Uuid::parse_str(user_id)?;
            sqlx::query_as::<_, App>(
                "SELECT * FROM apps WHERE user_id = $1 ORDER BY created_at DESC",
            )
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(apps)
    }

    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE apps SET active_deployment_id = $1, updated_at = NOW() WHERE id = $2")
            .bind(deployment_id)
            .bind(app_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn update_app_port(&self, id: Uuid, port: i32) -> anyhow::Result<()> {
        sqlx::query("UPDATE apps SET port = $1, updated_at = NOW() WHERE id = $2")
            .bind(port)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn create_deployment(&self, data: NewDeployment) -> anyhow::Result<Deployment> {
        let uid = Uuid::parse_str(&data.user_id)?;
        let env_json = serde_json::to_value(data.env_vars)?;

        let deployment = sqlx::query_as::<_, Deployment>(
            r#"
            INSERT INTO deployments (app_id, user_id, status, vcpus, memory_mib, disk_mib, port, env_vars)
            VALUES ($1, $2, 'PENDING', $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(data.app_id)
        .bind(uid)
        .bind(data.vcpus)
        .bind(data.memory_mib)
        .bind(data.disk_mib)
        .bind(data.port)
        .bind(env_json)
        .fetch_one(&self.pool)
        .await?;

        Ok(deployment)
    }

    async fn update_deployment_status(
        &self,
        id: Uuid,
        status: &str,
        job_id: Option<String>,
        image_tag: Option<String>,
        build_id: Option<String>,
        ip_address: Option<String>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE deployments SET status = $1, job_id = COALESCE($2, job_id), image_tag = COALESCE($3, image_tag), build_id = COALESCE($4, build_id), ip_address = COALESCE($5, ip_address), updated_at = NOW() WHERE id = $6"
        )
        .bind(status)
        .bind(job_id)
        .bind(image_tag)
        .bind(build_id)
        .bind(ip_address)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_deployment_port(&self, id: Uuid, port: i32) -> anyhow::Result<()> {
        sqlx::query("UPDATE deployments SET port = $1, updated_at = NOW() WHERE id = $2")
            .bind(port)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_deployment(&self, id: Uuid) -> anyhow::Result<Option<Deployment>> {
        let deployment = sqlx::query_as::<_, Deployment>("SELECT * FROM deployments WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(deployment)
    }

    async fn get_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<Option<Deployment>> {
        let deployment =
            sqlx::query_as::<_, Deployment>("SELECT * FROM deployments WHERE job_id = $1")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(deployment)
    }

    async fn list_deployments_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<Deployment>> {
        let deployments = sqlx::query_as::<_, Deployment>(
            "SELECT * FROM deployments WHERE app_id = $1 ORDER BY created_at DESC",
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(deployments)
    }

    async fn list_deployments_by_user(&self, user_id: &str) -> anyhow::Result<Vec<Deployment>> {
        let deployments = if user_id == "all" {
            sqlx::query_as::<_, Deployment>("SELECT * FROM deployments ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?
        } else {
            let uid = Uuid::parse_str(user_id)?;
            sqlx::query_as::<_, Deployment>(
                "SELECT * FROM deployments WHERE user_id = $1 ORDER BY created_at DESC",
            )
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(deployments)
    }

    async fn delete_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM deployments WHERE job_id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::postgres_user_repository::PostgresUserRepository;
    use crate::repositories::user_repository::NewUser;
    use crate::repositories::user_repository::UserRepository;
    use crate::repositories::user_repository::UserRole;

    async fn get_test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        });
        PgPool::connect(&url)
            .await
            .expect("failed to connect to test db")
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_app_lifecycle() {
        let pool = get_test_pool().await;
        let user_repo = PostgresUserRepository::new(pool.clone());
        let app_repo = PostgresAppRepository::new(pool.clone());

        // 1. Create a user first
        let email = format!("app_test_{}@example.com", Uuid::new_v4());
        let user_id = user_repo
            .create(NewUser {
                email: email.clone(),
                password_hash: "pass".into(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
            })
            .await
            .expect("failed to create user");

        // 2. Create an app
        let app_name = "test-app";
        let git_url = "https://github.com/test/repo";
        let app = app_repo
            .create_app(app_name, git_url, 80, None, &user_id.to_string(), None)
            .await
            .expect("failed to create app");
        assert_eq!(app.name, app_name);
        assert_eq!(app.git_url, git_url);
        assert_eq!(app.port, 80);

        // 3. List apps
        let apps = app_repo
            .list_apps_by_user(&user_id.to_string())
            .await
            .expect("failed to list apps");
        assert!(apps.iter().any(|a| a.id == app.id));

        // 4. Create a deployment
        let deployment = app_repo
            .create_deployment(crate::repositories::app_repository::NewDeployment {
                app_id: app.id,
                user_id: user_id.to_string(),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                env_vars: std::collections::HashMap::new(),
            })
            .await
            .expect("failed to create deployment");
        assert_eq!(deployment.status, "PENDING");

        // 5. Update deployment
        app_repo
            .update_deployment_status(
                deployment.id,
                "RUNNING",
                Some("job-123".to_string()),
                Some("img:v1".to_string()),
                Some("build-abc".to_string()),
                Some("10.0.0.1".to_string()),
            )
            .await
            .expect("failed to update deployment");

        let updated = app_repo
            .get_deployment(deployment.id)
            .await
            .expect("failed to get deployment")
            .expect("not found");
        assert_eq!(updated.status, "RUNNING");
        assert_eq!(updated.job_id.as_deref(), Some("job-123"));
        assert_eq!(updated.image_tag.as_deref(), Some("img:v1"));
        assert_eq!(updated.build_id.as_deref(), Some("build-abc"));
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_get_app_by_name() {
        let pool = get_test_pool().await;
        let app_repo = PostgresAppRepository::new(pool.clone());
        let user_id = Uuid::new_v4();
        let name = format!("name-test-{}", Uuid::new_v4());

        // Create app
        app_repo
            .create_app(&name, "git", 8080, None, &user_id.to_string(), None)
            .await
            .unwrap();

        // Get by name
        let app = app_repo.get_app_by_name(&name).await.unwrap().unwrap();
        assert_eq!(app.name, name);
        assert_eq!(app.user_id, user_id);

        // Cleanup
        app_repo.delete_app(app.id).await.unwrap();
    }
}
