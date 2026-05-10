#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use mikrom_scheduler::domain::{Job, JobRepository, VmConfig, Worker, WorkerRepository};
    use mikrom_scheduler::infrastructure::db::{PgJobRepository, PgWorkerRepository};

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

        // 3. Verify job is present
        let job = job_repo.get_job(&job_id).await.unwrap();
        assert!(job.is_some());

        // 4. Delete the job
        job_repo.remove_job(&job_id).await.unwrap();

        // 5. Verify job is gone
        let job = job_repo.get_job(&job_id).await.unwrap();
        assert!(job.is_none());
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
        let jobs = job_repo.list_jobs(None, None, None).await.unwrap();
        assert_eq!(jobs.len(), 4);

        // Perform bulk cleanup
        job_repo.remove_jobs_by_app(&app_id).await.unwrap();

        // Verify result
        let remaining_jobs = job_repo.list_jobs(None, None, None).await.unwrap();
        assert_eq!(remaining_jobs.len(), 1, "Only 1 job should remain");
        assert_eq!(
            remaining_jobs[0].app_id, "app-other",
            "The job from the other app should remain"
        );
    }
}
