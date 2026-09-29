use rust_agent::{
    providers::types::Provider,
    quota::{
        self,
        tracker::{
            create_memory_pool, get_recent_requests, get_today_quota_usage, increment_quota_usage,
            log_request,
        },
        QuotaManager,
    },
};

#[tokio::test]
async fn test_quota_increment_and_persistence() {
    let pool = create_memory_pool().unwrap();
    quota::run_migrations(&pool).await.unwrap();

    // Increment quota usage for providers
    increment_quota_usage(&pool, "groq", 1200).await.unwrap();
    increment_quota_usage(&pool, "groq", 800).await.unwrap();
    increment_quota_usage(&pool, "together", 500).await.unwrap();

    let groq_usage = get_today_quota_usage(&pool, "groq").await.unwrap();
    assert_eq!(groq_usage, 2000);

    let together_usage = get_today_quota_usage(&pool, "together").await.unwrap();
    assert_eq!(together_usage, 500);

    let unknown_usage = get_today_quota_usage(&pool, "unknown_provider")
        .await
        .unwrap();
    assert_eq!(unknown_usage, 0);

    // Test request logs
    log_request(&pool, "groq", "llama-3-8b-8192", 100, 50, 150, 120, 200)
        .await
        .unwrap();

    log_request(&pool, "together", "mistral-7b", 200, 80, 280, 350, 500)
        .await
        .unwrap();

    let logs = get_recent_requests(&pool, 10).await.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].provider_id, "together");
    assert_eq!(logs[0].status_code, 500);
    assert_eq!(logs[1].provider_id, "groq");
    assert_eq!(logs[1].status_code, 200);
}

#[tokio::test]
async fn test_quota_manager_enforcement_flow() {
    let pool = create_memory_pool().unwrap();
    quota::run_migrations(&pool).await.unwrap();

    let provider = Provider::new(
        "test_provider".to_string(),
        "Test Provider".to_string(),
        "https://api.test.com".to_string(),
        "api_key".to_string(),
        Some(500),
        None,
        30,
        vec!["test-model".to_string()],
    );

    // Initial check: 300 fits within 500 limit
    let allowed = QuotaManager::check_quota(&pool, &provider, 300)
        .await
        .unwrap();
    assert!(allowed);

    QuotaManager::record_usage(&pool, &provider.id, 300)
        .await
        .unwrap();

    // Now used is 300, requesting 250 more exceeds 500 (300 + 250 > 500)
    let allowed_after = QuotaManager::check_quota(&pool, &provider, 250)
        .await
        .unwrap();
    assert!(!allowed_after);

    // Requesting 150 more fits (300 + 150 <= 500)
    let allowed_fits = QuotaManager::check_quota(&pool, &provider, 150)
        .await
        .unwrap();
    assert!(allowed_fits);
}
