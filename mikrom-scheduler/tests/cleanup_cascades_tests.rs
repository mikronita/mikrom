#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use mikrom_scheduler::domain::{Job, JobRepository, VmConfig, Worker, WorkerRepository};
    use mikrom_scheduler::infrastructure::db::ipam::Ipam;
    use mikrom_scheduler::infrastructure::db::{PgJobRepository, PgWorkerRepository};
    use sqlx::Row;

    #[tokio::test]
    async fn test_cascading_cleanup_on_job_deletion() {
        let db = common_utils::TestDb::new().await;
        let pool = db.pool().clone();

        let job_repo = PgJobRepository::new(pool.clone());
        let worker_repo = PgWorkerRepository::new(pool.clone());

        // 1. Setup a worker
        let host_id = "test-host".to_string();
        let worker = Worker {
            host_id: host_id.clone(),
            hostname: "test-hostname".to_string(),
            ip_address: "192.168.1.1".to_string(),
            bridge_ip: "10.0.0.1/24".to_string(),
            wireguard_pubkey: None,
            wireguard_ip: None,
            wireguard_port: None,
            metrics: None,
            registered_at: 0,
            last_heartbeat: 0,
        };
        worker_repo.register(worker).await.unwrap();

        // 2. Add a job
        let job_id = "job-1".to_string();
        let app_id = "app-1".to_string();
        let job = Job::new(
            job_id.clone(),
            app_id.clone(),
            "test-app".to_string(),
            "alpine:latest".to_string(),
            VmConfig::default(),
            "user-1".to_string(),
            None,
        );
        job_repo.add_job(job).await.unwrap();

        // 3. Allocate an IP for this job
        let ipam = Ipam::new(pool.clone(), host_id.clone(), "10.0.0.1/24".to_string());
        let _allocation = ipam.allocate(&job_id).await.unwrap().unwrap();

        // 4. Verify IP is allocated and linked to job
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM ip_allocations WHERE job_id = $1")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "IP allocation should be linked to job_id");

        // 5. Delete the job and verify IP allocation is cleaned up automatically (CASCADE)
        job_repo.remove_job(&job_id).await.unwrap();

        let count: i64 = sqlx::query("SELECT COUNT(*) FROM ip_allocations WHERE job_id = $1")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            count, 0,
            "IP allocation should be cleaned up via CASCADE after job deletion"
        );
    }

    #[tokio::test]
    async fn test_remove_jobs_by_app() {
        let db = common_utils::TestDb::new().await;
        let pool = db.pool().clone();
        let job_repo = PgJobRepository::new(pool.clone());

        let app_id = "app-cleanup-test".to_string();

        // Add 3 jobs for the same app
        for i in 1..=3 {
            let job = Job::new(
                format!("job-{}", i),
                app_id.clone(),
                "test-app".to_string(),
                "alpine".to_string(),
                VmConfig::default(),
                "user-1".to_string(),
                None,
            );
            job_repo.add_job(job).await.unwrap();
        }

        // Add 1 job for a different app
        let other_job = Job::new(
            "job-other".to_string(),
            "app-other".to_string(),
            "other-app".to_string(),
            "alpine".to_string(),
            VmConfig::default(),
            "user-1".to_string(),
            None,
        );
        job_repo.add_job(other_job).await.unwrap();

        // Verify initial state
        let jobs = job_repo.list_jobs(None, None).await.unwrap();
        assert_eq!(jobs.len(), 4);

        // Perform bulk cleanup
        job_repo.remove_jobs_by_app(&app_id).await.unwrap();

        // Verify result
        let remaining_jobs = job_repo.list_jobs(None, None).await.unwrap();
        assert_eq!(remaining_jobs.len(), 1, "Only 1 job should remain");
        assert_eq!(
            remaining_jobs[0].app_id, "app-other",
            "The job from the other app should remain"
        );
    }
}
