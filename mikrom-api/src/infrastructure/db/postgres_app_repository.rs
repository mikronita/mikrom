use crate::domain::app::{App, Deployment, SecurityRule};
use crate::domain::app::{AppRepository, CreateAppParams, NewDeployment, UpdateDeploymentParams};
use crate::domain::error::DomainResult;
use crate::infrastructure::db::models::{DbApp, DbDeployment, DbSecurityRule};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresAppRepository {
    pool: PgPool,
    master_key: String,
}

impl PostgresAppRepository {
    pub fn new(pool: PgPool, master_key: String) -> Self {
        Self { pool, master_key }
    }

    fn decrypt_app(&self, mut app: App) -> DomainResult<App> {
        if let Some(ref encrypted) = app.github_webhook_secret {
            match crate::crypto::decrypt(encrypted, &self.master_key) {
                Ok(decrypted) => {
                    app.github_webhook_secret = Some(decrypted);
                },
                Err(e) => {
                    tracing::error!(
                        app_id = %app.id,
                        error = ?e,
                        "Failed to decrypt github_webhook_secret. Data might be corrupted or MASTER_KEY is incorrect."
                    );
                    return Err(crate::domain::DomainError::Infrastructure(format!(
                        "Failed to decrypt application secret: {}",
                        e
                    )));
                },
            }
        }
        Ok(app)
    }

    fn decrypt_deployment(&self, mut deployment: Deployment) -> DomainResult<Deployment> {
        if let serde_json::Value::String(ref encrypted) = deployment.env_vars {
            match crate::crypto::decrypt(encrypted, &self.master_key) {
                Ok(decrypted_raw) => match serde_json::from_str(&decrypted_raw) {
                    Ok(parsed) => {
                        deployment.env_vars = parsed;
                    },
                    Err(e) => {
                        tracing::error!(
                            deployment_id = %deployment.id,
                            error = ?e,
                            "Failed to parse decrypted env_vars JSON."
                        );
                        return Err(crate::domain::DomainError::Infrastructure(format!(
                            "Failed to parse decrypted environment variables: {}",
                            e
                        )));
                    },
                },
                Err(e) => {
                    tracing::error!(
                        deployment_id = %deployment.id,
                        error = ?e,
                        "Failed to decrypt env_vars. Data might be corrupted or MASTER_KEY is incorrect."
                    );
                    return Err(crate::domain::DomainError::Infrastructure(format!(
                        "Failed to decrypt deployment environment variables: {}",
                        e
                    )));
                },
            }
        }
        Ok(deployment)
    }
}

#[async_trait]
impl AppRepository for PostgresAppRepository {
    async fn create_app(&self, params: CreateAppParams) -> DomainResult<App> {
        let encrypted_secret = if let Some(secret) = params.github_webhook_secret {
            Some(crate::crypto::encrypt(&secret, &self.master_key)?)
        } else {
            None
        };

        let health_check_path = params.health_check_path.unwrap_or_else(|| "/".to_string());
        let drain_timeout = params.drain_timeout.unwrap_or(10);
        let desired_replicas = params.desired_replicas.unwrap_or(1);
        let min_replicas = params.min_replicas.unwrap_or(0);
        let max_replicas = params.max_replicas.unwrap_or(1);
        let autoscaling_enabled = params.autoscaling_enabled.unwrap_or(false);
        let cpu_threshold = params.cpu_threshold.unwrap_or(80.0);
        let mem_threshold = params.mem_threshold.unwrap_or(80.0);

        let result = sqlx::query_as::<_, DbApp>(
            "INSERT INTO apps (name, git_url, port, hostname, user_id, github_webhook_secret, github_installation_id, github_repo_id, github_repo_full_name, health_check_path, drain_timeout, desired_replicas, min_replicas, max_replicas, autoscaling_enabled, cpu_threshold, mem_threshold) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) RETURNING *"
        )
        .bind(&params.name)
        .bind(&params.git_url)
        .bind(i32::from(params.port))
        .bind(&params.hostname)
        .bind(params.user_id)
        .bind(encrypted_secret)
        .bind(params.github_installation_id)
        .bind(params.github_repo_id)
        .bind(&params.github_repo_full_name)
        .bind(health_check_path)
        .bind(drain_timeout)
        .bind(desired_replicas)
        .bind(min_replicas)
        .bind(max_replicas)
        .bind(autoscaling_enabled)
        .bind(cpu_threshold)
        .bind(mem_threshold)
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(db_app) => self.decrypt_app(db_app.into()),
            Err(e) => {
                if let Some(db_err) = e.as_database_error()
                    && db_err.code().as_deref() == Some("23505")
                {
                    return Err(crate::domain::DomainError::Infrastructure(format!(
                        "Application name '{}' is already taken",
                        params.name
                    )));
                }
                Err(e.into())
            },
        }
    }

    async fn get_app(&self, id: Uuid) -> DomainResult<Option<App>> {
        let db_app = sqlx::query_as::<_, DbApp>("SELECT * FROM apps WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        match db_app {
            Some(a) => Ok(Some(self.decrypt_app(a.into())?)),
            None => Ok(None),
        }
    }

    async fn get_app_by_name(&self, name: &str) -> DomainResult<Option<App>> {
        let db_app = sqlx::query_as::<_, DbApp>("SELECT * FROM apps WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        match db_app {
            Some(a) => Ok(Some(self.decrypt_app(a.into())?)),
            None => Ok(None),
        }
    }

    async fn get_app_by_github_repo_id(&self, repo_id: i64) -> DomainResult<Option<App>> {
        let db_app = sqlx::query_as::<_, DbApp>("SELECT * FROM apps WHERE github_repo_id = $1")
            .bind(repo_id)
            .fetch_optional(&self.pool)
            .await?;

        match db_app {
            Some(a) => Ok(Some(self.decrypt_app(a.into())?)),
            None => Ok(None),
        }
    }

    async fn delete_app(&self, id: Uuid) -> DomainResult<()> {
        sqlx::query("DELETE FROM apps WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn list_apps_by_user(&self, user_id: Option<Uuid>) -> DomainResult<Vec<App>> {
        let db_apps = match user_id {
            None => {
                sqlx::query_as::<_, DbApp>("SELECT * FROM apps ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await?
            },
            Some(uid) => {
                sqlx::query_as::<_, DbApp>(
                    "SELECT * FROM apps WHERE user_id = $1 ORDER BY created_at DESC",
                )
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            },
        };

        let mut decrypted_apps = Vec::with_capacity(db_apps.len());
        for db_app in db_apps {
            decrypted_apps.push(self.decrypt_app(db_app.into())?);
        }
        Ok(decrypted_apps)
    }

    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> DomainResult<()> {
        sqlx::query("UPDATE apps SET active_deployment_id = $1, updated_at = NOW() WHERE id = $2")
            .bind(deployment_id)
            .bind(app_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn update_app_port(
        &self,
        id: Uuid,
        port: crate::domain::types::Port,
    ) -> DomainResult<()> {
        sqlx::query("UPDATE apps SET port = $1, updated_at = NOW() WHERE id = $2")
            .bind(i32::from(port))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_app_scaling(&self, id: Uuid, desired_replicas: i32) -> DomainResult<()> {
        sqlx::query("UPDATE apps SET desired_replicas = $1, updated_at = NOW() WHERE id = $2")
            .bind(desired_replicas)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_app_autoscaling(
        &self,
        id: Uuid,
        min_replicas: i32,
        max_replicas: i32,
        enabled: bool,
        cpu_threshold: Option<f64>,
        mem_threshold: Option<f64>,
    ) -> DomainResult<()> {
        sqlx::query(
            "UPDATE apps SET min_replicas = $1, max_replicas = $2, autoscaling_enabled = $3, \
             cpu_threshold = COALESCE($4, cpu_threshold), mem_threshold = COALESCE($5, mem_threshold), \
             updated_at = NOW() WHERE id = $6",
        )
        .bind(min_replicas)
        .bind(max_replicas)
        .bind(enabled)
        .bind(cpu_threshold)
        .bind(mem_threshold)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_deployment(&self, data: NewDeployment) -> DomainResult<Deployment> {
        let uid = Uuid::parse_str(&data.user_id)?;

        // Encrypt env_vars
        let env_raw = serde_json::to_string(&data.env_vars)?;
        let encrypted_env = crate::crypto::encrypt(&env_raw, &self.master_key)?;
        let env_json = serde_json::Value::String(encrypted_env);

        let db_deployment = sqlx::query_as::<_, DbDeployment>(
            r#"
            INSERT INTO deployments (app_id, user_id, status, vcpus, memory_mib, disk_mib, port, env_vars, trigger_source, git_commit_hash, git_commit_message, git_branch, hypervisor)
            VALUES ($1, $2, 'BUILDING', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(data.app_id)
        .bind(uid)
        .bind(i32::from(data.vcpus))
        .bind(i64::from(i32::from(data.memory_mib)))
        .bind(data.disk_mib)
        .bind(i32::from(data.port))
        .bind(env_json)
        .bind(data.trigger_source)
        .bind(data.git_commit_hash)
        .bind(data.git_commit_message)
        .bind(data.git_branch)
        .bind(data.hypervisor)
        .fetch_one(&self.pool)
        .await?;

        self.decrypt_deployment(db_deployment.into())
    }

    async fn update_deployment(
        &self,
        id: Uuid,
        params: UpdateDeploymentParams,
    ) -> DomainResult<()> {
        sqlx::query(
            "UPDATE deployments SET status = COALESCE($1, status), job_id = COALESCE($2, job_id), image_tag = COALESCE($3, image_tag), build_id = COALESCE($4, build_id), ipv6_address = COALESCE($5, ipv6_address), git_commit_hash = COALESCE($6, git_commit_hash), git_commit_message = COALESCE($7, git_commit_message), git_branch = COALESCE($8, git_branch), updated_at = NOW() WHERE id = $9"
        )
        .bind(params.status)
        .bind(params.job_id)
        .bind(params.image_tag)
        .bind(params.build_id)
        .bind(params.ipv6_address)
        .bind(params.git_commit_hash)
        .bind(params.git_commit_message)
        .bind(params.git_branch)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_deployment_port(
        &self,
        id: Uuid,
        port: crate::domain::types::Port,
    ) -> DomainResult<()> {
        sqlx::query("UPDATE deployments SET port = $1, updated_at = NOW() WHERE id = $2")
            .bind(i32::from(port))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_deployment(&self, id: Uuid) -> DomainResult<Option<Deployment>> {
        let db_deployment =
            sqlx::query_as::<_, DbDeployment>("SELECT * FROM deployments WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        match db_deployment {
            Some(d) => Ok(Some(self.decrypt_deployment(d.into())?)),
            None => Ok(None),
        }
    }

    async fn get_deployment_by_job_id(&self, job_id: &str) -> DomainResult<Option<Deployment>> {
        let db_deployment =
            sqlx::query_as::<_, DbDeployment>("SELECT * FROM deployments WHERE job_id = $1")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await?;

        match db_deployment {
            Some(d) => Ok(Some(self.decrypt_deployment(d.into())?)),
            None => Ok(None),
        }
    }

    async fn list_deployments_by_app(&self, app_id: Uuid) -> DomainResult<Vec<Deployment>> {
        let db_deployments = sqlx::query_as::<_, DbDeployment>(
            "SELECT * FROM deployments WHERE app_id = $1 ORDER BY created_at DESC",
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await?;

        let mut decrypted_deps = Vec::with_capacity(db_deployments.len());
        for db_dep in db_deployments {
            decrypted_deps.push(self.decrypt_deployment(db_dep.into())?);
        }
        Ok(decrypted_deps)
    }

    async fn list_deployments_by_user(
        &self,
        user_id: Option<Uuid>,
    ) -> DomainResult<Vec<Deployment>> {
        let db_deployments = match user_id {
            None => {
                sqlx::query_as::<_, DbDeployment>(
                    "SELECT * FROM deployments ORDER BY created_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            },
            Some(uid) => {
                sqlx::query_as::<_, DbDeployment>(
                    "SELECT * FROM deployments WHERE user_id = $1 ORDER BY created_at DESC",
                )
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            },
        };

        let mut decrypted_deps = Vec::with_capacity(db_deployments.len());
        for db_dep in db_deployments {
            decrypted_deps.push(self.decrypt_deployment(db_dep.into())?);
        }
        Ok(decrypted_deps)
    }

    async fn get_active_deployment(&self, app_id: Uuid) -> DomainResult<Option<Deployment>> {
        let db_deployment = sqlx::query_as::<_, DbDeployment>(
            "SELECT d.* FROM deployments d JOIN apps a ON d.id = a.active_deployment_id WHERE a.id = $1"
        )
        .bind(app_id)
        .fetch_optional(&self.pool)
        .await?;

        match db_deployment {
            Some(d) => Ok(Some(self.decrypt_deployment(d.into())?)),
            None => Ok(None),
        }
    }

    async fn delete_deployment_by_job_id(&self, job_id: &str) -> DomainResult<()> {
        sqlx::query("DELETE FROM deployments WHERE job_id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn list_security_rules(&self, app_id: Uuid) -> DomainResult<Vec<SecurityRule>> {
        let db_rules = sqlx::query_as::<_, DbSecurityRule>(
            "SELECT * FROM security_rules WHERE app_id = $1 ORDER BY priority ASC, created_at ASC",
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(db_rules.into_iter().map(|r| r.into()).collect())
    }

    async fn create_security_rule(
        &self,
        app_id: Uuid,
        protocol: String,
        port_start: crate::domain::types::Port,
        port_end: crate::domain::types::Port,
        action: String,
    ) -> DomainResult<SecurityRule> {
        let db_rule = sqlx::query_as::<_, DbSecurityRule>(
            "INSERT INTO security_rules (app_id, protocol, port_start, port_end, action) VALUES ($1, $2, $3, $4, $5) RETURNING *"
        )
        .bind(app_id)
        .bind(protocol)
        .bind(i32::from(port_start))
        .bind(i32::from(port_end))
        .bind(action)
        .fetch_one(&self.pool)
        .await?;

        Ok(db_rule.into())
    }

    async fn delete_security_rule(&self, id: Uuid) -> DomainResult<()> {
        sqlx::query("DELETE FROM security_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::{NewUser, UserRepository, UserRole};
    use crate::infrastructure::db::PostgresUserRepository;
    use crate::test_utils::TestDb;

    #[tokio::test]
    async fn test_app_lifecycle() {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let user_repo = PostgresUserRepository::new(pool.clone());
        let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".into());

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
            .create_app(CreateAppParams {
                name: app_name.to_string(),
                git_url: git_url.to_string(),
                port: crate::domain::types::Port::new(80).unwrap(),
                hostname: None,
                user_id,
                github_webhook_secret: None,
                github_installation_id: None,
                github_repo_id: None,
                github_repo_full_name: None,
                health_check_path: None,
                drain_timeout: None,
                ..Default::default()
            })
            .await
            .expect("failed to create app");
        assert_eq!(app.name, app_name);
        assert_eq!(app.git_url, git_url);
        assert_eq!(app.port, 80);

        // 3. List apps
        let apps = app_repo
            .list_apps_by_user(Some(user_id))
            .await
            .expect("failed to list apps");
        assert!(apps.iter().any(|a| a.id == app.id));

        // 4. Create a deployment
        let deployment = app_repo
            .create_deployment(NewDeployment::from_handler(
                app.id,
                user_id.to_string(),
                crate::domain::types::CpuCores::new(1).unwrap(),
                crate::domain::types::MemoryMb::new(256).unwrap(),
                1024,
                crate::domain::types::Port::new(8080).unwrap(),
                std::collections::HashMap::new(),
                "manual".to_string(),
                None,
                0,
            ))
            .await
            .expect("failed to create deployment");
        assert_eq!(deployment.status, "BUILDING");

        // 5. Update deployment
        app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("RUNNING".to_string()),
                    job_id: Some("job-123".to_string()),
                    image_tag: Some("img:v1".to_string()),
                    build_id: Some("build-abc".to_string()),
                    ..Default::default()
                },
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
    async fn test_get_app_by_name() {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let user_repo = PostgresUserRepository::new(pool.clone());
        let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".into());

        // Create a user first to satisfy FK constraint
        let email = format!("app_name_test_{}@example.com", Uuid::new_v4());
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

        let name = format!("name-test-{}", Uuid::new_v4());

        // Create app
        app_repo
            .create_app(CreateAppParams {
                name: name.clone(),
                git_url: "git".to_string(),
                port: crate::domain::types::Port::new(8080).unwrap(),
                hostname: None,
                user_id,
                github_webhook_secret: None,
                github_installation_id: None,
                github_repo_id: None,
                github_repo_full_name: None,
                health_check_path: None,
                drain_timeout: None,
                ..Default::default()
            })
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
