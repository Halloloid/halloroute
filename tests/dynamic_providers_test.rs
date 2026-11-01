/// Integration tests for Phase 5.3: Dynamic Provider Management
use halloroute::providers::{
    create_dynamic_provider, delete_dynamic_provider, get_dynamic_provider, list_dynamic_providers,
    parse_free_model_filter, record_to_provider, serialize_free_model_filter,
    update_dynamic_provider, DynamicProviderConfig, FreeModelFilter,
};
use halloroute::quota;

#[tokio::test]
async fn test_parse_and_serialize_free_model_filters() {
    // Test "all" filter
    let filter = parse_free_model_filter("all").unwrap();
    assert!(matches!(filter, FreeModelFilter::All));
    assert_eq!(serialize_free_model_filter(&filter), "all");

    // Test "none" filter
    let filter = parse_free_model_filter("none").unwrap();
    assert!(matches!(filter, FreeModelFilter::None));
    assert_eq!(serialize_free_model_filter(&filter), "none");

    // Test patterns filter
    let filter = parse_free_model_filter("patterns:llama*,mistral*,gpt-4*").unwrap();
    if let FreeModelFilter::Patterns(patterns) = &filter {
        assert_eq!(patterns.len(), 3);
        assert!(patterns.contains(&"llama*".to_string()));
        assert!(patterns.contains(&"mistral*".to_string()));
        assert!(patterns.contains(&"gpt-4*".to_string()));
    } else {
        panic!("Expected Patterns variant");
    }
    assert_eq!(
        serialize_free_model_filter(&filter),
        "patterns:llama*,mistral*,gpt-4*"
    );

    // Test explicit list filter
    let filter = parse_free_model_filter("explicit:model-a,model-b,model-c").unwrap();
    if let FreeModelFilter::ExplicitList(models) = &filter {
        assert_eq!(models.len(), 3);
        assert!(models.contains(&"model-a".to_string()));
    } else {
        panic!("Expected ExplicitList variant");
    }
    assert_eq!(
        serialize_free_model_filter(&filter),
        "explicit:model-a,model-b,model-c"
    );

    // Test invalid filter
    let result = parse_free_model_filter("invalid-format");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_dynamic_provider() {
    let pool = quota::create_memory_pool().unwrap();

    let config = DynamicProviderConfig {
        id: "test-provider".to_string(),
        name: "Test Provider".to_string(),
        api_endpoint: "https://api.test.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("test-key-12345".to_string()),
        free_quota_daily: Some(100000),
        free_quota_monthly: None,
        rate_limit_rpm: 30,
        free_model_filter: "all".to_string(),
    };

    let models = vec!["test-model-1".to_string(), "test-model-2".to_string()];

    let record = create_dynamic_provider(&pool, config.clone(), models.clone())
        .await
        .unwrap();

    assert_eq!(record.id, "test-provider");
    assert_eq!(record.name, "Test Provider");
    assert_eq!(record.api_endpoint, "https://api.test.com/v1");
    assert_eq!(record.auth_type, "bearer");
    assert_eq!(record.api_key, Some("test-key-12345".to_string()));
    assert_eq!(record.free_quota_daily, Some(100000));
    assert_eq!(record.rate_limit_rpm, 30);
    assert_eq!(record.models.len(), 2);
    assert!(record.is_active);
    assert!(!record.created_at.is_empty());
    assert!(!record.updated_at.is_empty());
}

#[tokio::test]
async fn test_list_dynamic_providers() {
    let pool = quota::create_memory_pool().unwrap();

    // Initially empty
    let providers = list_dynamic_providers(&pool).await.unwrap();
    assert_eq!(providers.len(), 0);

    // Create first provider
    let config1 = DynamicProviderConfig {
        id: "provider-1".to_string(),
        name: "Provider One".to_string(),
        api_endpoint: "https://api.one.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("key-1".to_string()),
        free_quota_daily: Some(50000),
        free_quota_monthly: None,
        rate_limit_rpm: 20,
        free_model_filter: "all".to_string(),
    };

    create_dynamic_provider(&pool, config1, vec!["model-1".to_string()])
        .await
        .unwrap();

    // Create second provider
    let config2 = DynamicProviderConfig {
        id: "provider-2".to_string(),
        name: "Provider Two".to_string(),
        api_endpoint: "https://api.two.com/v1".to_string(),
        auth_type: "none".to_string(),
        api_key: None,
        free_quota_daily: None,
        free_quota_monthly: Some(1_000_000),
        rate_limit_rpm: 60,
        free_model_filter: "patterns:llama*".to_string(),
    };

    create_dynamic_provider(&pool, config2, vec!["model-2".to_string()])
        .await
        .unwrap();

    // List all providers
    let providers = list_dynamic_providers(&pool).await.unwrap();
    assert_eq!(providers.len(), 2);

    // Most recent first (provider-2)
    assert_eq!(providers[0].id, "provider-2");
    assert_eq!(providers[1].id, "provider-1");
}

#[tokio::test]
async fn test_get_dynamic_provider() {
    let pool = quota::create_memory_pool().unwrap();

    let config = DynamicProviderConfig {
        id: "get-test".to_string(),
        name: "Get Test Provider".to_string(),
        api_endpoint: "https://api.gettest.com/v1".to_string(),
        auth_type: "api_key".to_string(),
        api_key: Some("test-key".to_string()),
        free_quota_daily: Some(75000),
        free_quota_monthly: None,
        rate_limit_rpm: 25,
        free_model_filter: "explicit:model-x,model-y".to_string(),
    };

    let models = vec!["model-x".to_string(), "model-y".to_string()];

    create_dynamic_provider(&pool, config, models)
        .await
        .unwrap();

    // Get existing provider
    let record = get_dynamic_provider(&pool, "get-test").await.unwrap();
    assert_eq!(record.id, "get-test");
    assert_eq!(record.name, "Get Test Provider");
    assert_eq!(record.models.len(), 2);

    // Get non-existent provider
    let result = get_dynamic_provider(&pool, "non-existent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_dynamic_provider() {
    let pool = quota::create_memory_pool().unwrap();

    // Create initial provider
    let config = DynamicProviderConfig {
        id: "update-test".to_string(),
        name: "Original Name".to_string(),
        api_endpoint: "https://api.original.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("original-key".to_string()),
        free_quota_daily: Some(50000),
        free_quota_monthly: None,
        rate_limit_rpm: 30,
        free_model_filter: "all".to_string(),
    };

    let original_models = vec!["model-1".to_string()];
    let _created = create_dynamic_provider(&pool, config, original_models)
        .await
        .unwrap();

    // Update provider
    let update_config = DynamicProviderConfig {
        id: "update-test".to_string(),
        name: "Updated Name".to_string(),
        api_endpoint: "https://api.updated.com/v1".to_string(),
        auth_type: "api_key".to_string(),
        api_key: Some("updated-key".to_string()),
        free_quota_daily: Some(100000),
        free_quota_monthly: Some(2_000_000),
        rate_limit_rpm: 60,
        free_model_filter: "patterns:gpt*".to_string(),
    };

    let updated_models = vec!["model-1".to_string(), "model-2".to_string()];
    let updated = update_dynamic_provider(&pool, "update-test", update_config, updated_models)
        .await
        .unwrap();

    assert_eq!(updated.id, "update-test");
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.api_endpoint, "https://api.updated.com/v1");
    assert_eq!(updated.auth_type, "api_key");
    assert_eq!(updated.api_key, Some("updated-key".to_string()));
    assert_eq!(updated.free_quota_daily, Some(100000));
    assert_eq!(updated.free_quota_monthly, Some(2_000_000));
    assert_eq!(updated.rate_limit_rpm, 60);
    assert_eq!(updated.models.len(), 2);
    assert_eq!(updated.free_model_filter, "patterns:gpt*");

    // Verify created_at is preserved (though we can't check exact value)
    // and updated_at is different
    let fetched = get_dynamic_provider(&pool, "update-test").await.unwrap();
    assert_eq!(fetched.name, "Updated Name");

    // Update non-existent provider
    let bad_config = DynamicProviderConfig {
        id: "non-existent".to_string(),
        name: "Bad".to_string(),
        api_endpoint: "https://bad.com".to_string(),
        auth_type: "none".to_string(),
        api_key: None,
        free_quota_daily: None,
        free_quota_monthly: None,
        rate_limit_rpm: 10,
        free_model_filter: "all".to_string(),
    };

    let result = update_dynamic_provider(&pool, "non-existent", bad_config, vec![]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_dynamic_provider() {
    let pool = quota::create_memory_pool().unwrap();

    // Create provider
    let config = DynamicProviderConfig {
        id: "delete-test".to_string(),
        name: "Delete Test".to_string(),
        api_endpoint: "https://api.delete.com/v1".to_string(),
        auth_type: "none".to_string(),
        api_key: None,
        free_quota_daily: None,
        free_quota_monthly: None,
        rate_limit_rpm: 30,
        free_model_filter: "all".to_string(),
    };

    create_dynamic_provider(&pool, config, vec!["model-1".to_string()])
        .await
        .unwrap();

    // Verify it exists
    let record = get_dynamic_provider(&pool, "delete-test").await.unwrap();
    assert_eq!(record.id, "delete-test");

    // Delete it
    delete_dynamic_provider(&pool, "delete-test").await.unwrap();

    // Verify it's gone
    let result = get_dynamic_provider(&pool, "delete-test").await;
    assert!(result.is_err());

    // Delete non-existent provider
    let result = delete_dynamic_provider(&pool, "non-existent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_record_to_provider_conversion() {
    let pool = quota::create_memory_pool().unwrap();

    let config = DynamicProviderConfig {
        id: "convert-test".to_string(),
        name: "Convert Test".to_string(),
        api_endpoint: "https://api.convert.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("convert-key".to_string()),
        free_quota_daily: Some(80000),
        free_quota_monthly: Some(1_500_000),
        rate_limit_rpm: 40,
        free_model_filter: "all".to_string(),
    };

    let models = vec![
        "model-a".to_string(),
        "model-b".to_string(),
        "model-c".to_string(),
    ];

    let record = create_dynamic_provider(&pool, config, models.clone())
        .await
        .unwrap();

    // Convert to Provider
    let provider = record_to_provider(&record);

    assert_eq!(provider.id, "convert-test");
    assert_eq!(provider.name, "Convert Test");
    assert_eq!(provider.api_endpoint, "https://api.convert.com/v1");
    assert_eq!(provider.auth_type, "bearer");
    assert_eq!(provider.free_quota_daily, Some(80000));
    assert_eq!(provider.free_quota_monthly, Some(1_500_000));
    assert_eq!(provider.rate_limit_rpm, 40);
    assert_eq!(provider.models.len(), 3);
    assert!(provider.models.contains(&"model-a".to_string()));
    assert!(provider.models.contains(&"model-b".to_string()));
    assert!(provider.models.contains(&"model-c".to_string()));
    assert!(provider.is_available());
}

#[tokio::test]
async fn test_multiple_providers_lifecycle() {
    let pool = quota::create_memory_pool().unwrap();

    // Create 3 providers
    for i in 1..=3 {
        let config = DynamicProviderConfig {
            id: format!("provider-{}", i),
            name: format!("Provider {}", i),
            api_endpoint: format!("https://api-{}.com/v1", i),
            auth_type: "bearer".to_string(),
            api_key: Some(format!("key-{}", i)),
            free_quota_daily: Some(i * 10000),
            free_quota_monthly: None,
            rate_limit_rpm: 30,
            free_model_filter: "all".to_string(),
        };

        create_dynamic_provider(&pool, config, vec![format!("model-{}", i)])
            .await
            .unwrap();
    }

    // List all
    let providers = list_dynamic_providers(&pool).await.unwrap();
    assert_eq!(providers.len(), 3);

    // Update provider-2
    let update_config = DynamicProviderConfig {
        id: "provider-2".to_string(),
        name: "Updated Provider 2".to_string(),
        api_endpoint: "https://api-2-updated.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("updated-key-2".to_string()),
        free_quota_daily: Some(200000),
        free_quota_monthly: None,
        rate_limit_rpm: 50,
        free_model_filter: "patterns:llama*".to_string(),
    };

    update_dynamic_provider(
        &pool,
        "provider-2",
        update_config,
        vec!["model-2-v2".to_string()],
    )
    .await
    .unwrap();

    // Verify update
    let updated = get_dynamic_provider(&pool, "provider-2").await.unwrap();
    assert_eq!(updated.name, "Updated Provider 2");
    assert_eq!(updated.models[0], "model-2-v2");

    // Delete provider-1
    delete_dynamic_provider(&pool, "provider-1").await.unwrap();

    // List remaining
    let providers = list_dynamic_providers(&pool).await.unwrap();
    assert_eq!(providers.len(), 2);

    let ids: Vec<String> = providers.iter().map(|p| p.id.clone()).collect();
    assert!(ids.contains(&"provider-2".to_string()));
    assert!(ids.contains(&"provider-3".to_string()));
    assert!(!ids.contains(&"provider-1".to_string()));
}

#[tokio::test]
async fn test_provider_with_no_api_key() {
    let pool = quota::create_memory_pool().unwrap();

    let config = DynamicProviderConfig {
        id: "no-key-provider".to_string(),
        name: "No Key Provider".to_string(),
        api_endpoint: "https://api.nokey.com/v1".to_string(),
        auth_type: "none".to_string(),
        api_key: None,
        free_quota_daily: None,
        free_quota_monthly: None,
        rate_limit_rpm: 10,
        free_model_filter: "all".to_string(),
    };

    let record = create_dynamic_provider(&pool, config, vec!["free-model".to_string()])
        .await
        .unwrap();

    assert_eq!(record.auth_type, "none");
    assert!(record.api_key.is_none());
    assert!(record.free_quota_daily.is_none());
    assert!(record.free_quota_monthly.is_none());

    let provider = record_to_provider(&record);
    assert_eq!(provider.auth_type, "none");
    assert!(provider.resolve_api_key().is_none());
}

#[tokio::test]
async fn test_stored_api_key_reaches_runtime_provider_but_not_json() {
    let pool = quota::create_memory_pool().unwrap();

    let config = DynamicProviderConfig {
        id: "keyed-provider".to_string(),
        name: "Keyed".to_string(),
        api_endpoint: "https://api.keyed.com/v1".to_string(),
        auth_type: "bearer".to_string(),
        api_key: Some("super-secret".to_string()),
        free_quota_daily: None,
        free_quota_monthly: None,
        rate_limit_rpm: 10,
        free_model_filter: "all".to_string(),
    };

    let record = create_dynamic_provider(&pool, config, vec!["m".to_string()])
        .await
        .unwrap();

    // Survives a DB round trip and is used for upstream auth.
    let fetched = get_dynamic_provider(&pool, "keyed-provider").await.unwrap();
    let provider = record_to_provider(&fetched);
    assert_eq!(provider.resolve_api_key().as_deref(), Some("super-secret"));

    // But is never leaked through API/JSON serialization.
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("super-secret"));
    assert!(!serde_json::to_string(&provider)
        .unwrap()
        .contains("super-secret"));
}
