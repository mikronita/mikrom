use crate::domain::error::DomainResult;
use crate::domain::tenant::{Tenant, TenantMember, TenantRepository};
use crate::infrastructure::db::models::{DbTenant, DbTenantMember};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresTenantRepository {
    pool: PgPool,
}

impl PostgresTenantRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepository for PostgresTenantRepository {
    async fn create(&self, name: String, slug: String) -> DomainResult<Tenant> {
        let db_tenant = sqlx::query_as::<_, DbTenant>(
            r#"
            INSERT INTO tenants (tenant_id, name)
            VALUES ($1, $2)
            RETURNING id, tenant_id, name, created_at, updated_at
            "#,
        )
        .bind(slug)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;

        Ok(db_tenant.into())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<Tenant>> {
        let db_tenant = sqlx::query_as::<_, DbTenant>(
            "SELECT id, tenant_id, name, created_at, updated_at FROM tenants WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(db_tenant.map(Into::into))
    }

    async fn find_by_slug(&self, slug: &str) -> DomainResult<Option<Tenant>> {
        let db_tenant = sqlx::query_as::<_, DbTenant>(
            "SELECT id, tenant_id, name, created_at, updated_at FROM tenants WHERE tenant_id = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        Ok(db_tenant.map(Into::into))
    }

    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<Tenant>> {
        let db_tenants = sqlx::query_as::<_, DbTenant>(
            r#"
            SELECT t.id, t.tenant_id, t.name, t.created_at, t.updated_at
            FROM tenants t
            JOIN tenant_members tm ON t.id = tm.tenant_id
            WHERE tm.user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(db_tenants.into_iter().map(Into::into).collect())
    }

    async fn update(&self, tenant_id: Uuid, name: String) -> DomainResult<Tenant> {
        let db_tenant = sqlx::query_as::<_, DbTenant>(
            r#"
            UPDATE tenants
            SET name = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, tenant_id, name, created_at, updated_at
            "#,
        )
        .bind(tenant_id)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;

        Ok(db_tenant.into())
    }

    async fn list_all(&self) -> DomainResult<Vec<Tenant>> {
        let db_tenants = sqlx::query_as::<_, DbTenant>(
            "SELECT id, tenant_id, name, created_at, updated_at FROM tenants ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(db_tenants.into_iter().map(Into::into).collect())
    }

    async fn delete(&self, tenant_id: Uuid) -> DomainResult<bool> {
        let result = sqlx::query("DELETE FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn add_member(&self, tenant_id: Uuid, user_id: Uuid, role: &str) -> DomainResult<()> {
        sqlx::query("INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ($1, $2, $3)")
            .bind(tenant_id)
            .bind(user_id)
            .bind(role)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_members(&self, tenant_id: Uuid) -> DomainResult<Vec<TenantMember>> {
        let db_members = sqlx::query_as::<_, DbTenantMember>(
            "SELECT tenant_id, user_id, role FROM tenant_members WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(db_members.into_iter().map(Into::into).collect())
    }

    async fn is_member(&self, tenant_id: Uuid, user_id: Uuid) -> DomainResult<bool> {
        let result = sqlx::query(
            "SELECT 1 as one FROM tenant_members WHERE tenant_id = $1 AND user_id = $2",
        )
        .bind(tenant_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::{NewUser, UserRepository, UserRole};
    use crate::infrastructure::db::postgres_user_repository::PostgresUserRepository;
    use crate::test_utils::TestDb;

    #[tokio::test]
    #[ignore = "requires a configured PostgreSQL test database"]
    async fn test_tenant_lifecycle() {
        let Ok(db) = TestDb::try_new().await else {
            eprintln!("Skipping tenant repository test: database unavailable");
            return;
        };
        let pool = db.pool().clone();
        let repo = PostgresTenantRepository::new(pool.clone());
        let user_repo = PostgresUserRepository::new(pool.clone());

        // 1. Create a user
        let user_id = user_repo
            .create(NewUser {
                email: format!("tenant_test_{}@example.com", Uuid::new_v4()),
                password_hash: "hash".into(),
                role: UserRole::User,
                first_name: Some("Test".into()),
                last_name: Some("User".into()),
                avatar_url: None,
            })
            .await
            .unwrap();

        // 2. Create a tenant
        let slug = crate::domain::Tenant::generate_slug();
        let tenant = repo
            .create("Project Alpha".into(), slug.clone())
            .await
            .unwrap();
        assert_eq!(tenant.name, "Project Alpha");
        assert_eq!(tenant.tenant_id, slug);

        // 3. Add member
        repo.add_member(tenant.id, user_id, "admin").await.unwrap();

        // 4. Verify membership
        let is_member = repo.is_member(tenant.id, user_id).await.unwrap();
        assert!(is_member);

        let members = repo.get_members(tenant.id).await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, user_id);

        // 5. Find by slug
        let found = repo.find_by_slug(&slug).await.unwrap().unwrap();
        assert_eq!(found.id, tenant.id);

        // 6. List by user
        let user_tenants = repo.list_by_user(user_id).await.unwrap();
        assert!(user_tenants.iter().any(|t| t.id == tenant.id));

        // 7. Update tenant name
        let updated = repo.update(tenant.id, "Project Beta".into()).await.unwrap();
        assert_eq!(updated.name, "Project Beta");
        assert_eq!(updated.tenant_id, tenant.tenant_id);

        // 8. Delete tenant
        let deleted = repo.delete(tenant.id).await.unwrap();
        assert!(deleted);
    }
}
