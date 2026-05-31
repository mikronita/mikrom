use mikrom_api::AppState;
use uuid::Uuid;

#[tokio::test]
async fn test_concurrent_flows_prevented() {
    let state = AppState::default();
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();

    let guard1 = state
        .try_start_flow(app_id.into())
        .expect("first flow should start");
    assert!(
        state.try_start_flow(app_id.into()).is_none(),
        "second flow should be rejected while the first is active"
    );
    assert!(
        state.try_start_flow(other_app_id.into()).is_some(),
        "a different app should not be blocked by an active flow"
    );

    drop(guard1);

    assert!(
        state.try_start_flow(app_id.into()).is_some(),
        "flow should be allowed again after the guard is dropped"
    );
}
