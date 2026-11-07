use super::model::*;
use serde_json::json;
#[test]
fn connection_status_retains_omission_and_status_values() {
    let value = serde_json::to_value(ConnectionStatus {
        provider: Provider::Github,
        status: "pending".into(),
        label: None,
        message: None,
        client_id: None,
        api_id: None,
        user_code: None,
        poll_after_ms: Some(1000),
        qr_code: None,
        capabilities: vec![],
    })
    .unwrap();
    assert_eq!(
        value,
        json!({"provider":"github","status":"pending","pollAfterMs":1000,"capabilities":[]})
    );
}
#[test]
fn connection_page_retains_stable_values() {
    assert!(matches!(
        serde_json::from_str::<ConnectionPage>("\"vk-auth\""),
        Ok(ConnectionPage::VkAuth)
    ));
    assert!(serde_json::from_str::<ConnectionPage>("\"vkAuth\"").is_err());
}
#[test]
fn matches_shared_frontend_wire_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/shared/lib/fixtures/native-wire.json"
    ))
    .unwrap();
    let value: ConnectionConfig =
        serde_json::from_value(fixture["connectionConfig"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["connectionConfig"]
    );
    let value: ConnectionStatus =
        serde_json::from_value(fixture["connectionStatus"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["connectionStatus"]
    );
}
