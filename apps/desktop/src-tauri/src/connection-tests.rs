use super::{
    http::Http,
    model::{ConnectionConfig, Provider, Token},
    session::Connections,
    storage::{self, save_secret},
};
use serde_json::Value;
use std::sync::Arc;
struct NoNetwork;
impl Http for NoNetwork {
    fn get(&self, _: &str, _: &[(&str, String)], _: Option<&str>) -> Result<Value, String> {
        panic!("unexpected network")
    }
    fn post(&self, _: &str, _: &[(&str, String)]) -> Result<Value, String> {
        panic!("unexpected network")
    }
}
#[test]
fn source_token_is_bound_to_the_granted_account_even_after_reconnect() {
    let secrets = Arc::new(storage::MemorySecrets::default());
    save_secret(
        secrets.as_ref(),
        "github-token",
        &Token {
            access: "test-token".into(),
            refresh: None,
            device_id: None,
            expires_at: None,
            refreshed_at: 0,
            account_id: "new-account".into(),
            label: "test".into(),
        },
    )
    .unwrap();
    let hub = Connections::test(secrets, Arc::new(NoNetwork));
    assert!(hub.token_for(Provider::Github, "old-account").is_err());
    assert_eq!(
        hub.token_for(Provider::Github, "new-account")
            .unwrap()
            .account_id,
        "new-account"
    );
}
struct RefreshResponse(Result<Value, String>);
impl Http for RefreshResponse {
    fn get(&self, _: &str, _: &[(&str, String)], _: Option<&str>) -> Result<Value, String> {
        self.0.clone()
    }
    fn post(&self, _: &str, _: &[(&str, String)]) -> Result<Value, String> {
        self.0.clone()
    }
}
fn expired_connection(provider: Provider, response: Result<Value, String>) -> Connections {
    let secrets = Arc::new(storage::MemorySecrets::default());
    save_secret(
        secrets.as_ref(),
        &format!("{}-config", provider.key()),
        &ConnectionConfig {
            provider: Some(provider),
            client_id: Some("123".into()),
            ..ConnectionConfig::default()
        },
    )
    .unwrap();
    save_secret(
        secrets.as_ref(),
        &format!("{}-token", provider.key()),
        &Token {
            access: "expired-access".into(),
            refresh: Some("expired-refresh".into()),
            device_id: Some("device".into()),
            expires_at: Some(1),
            refreshed_at: 0,
            account_id: "42".into(),
            label: "fixture".into(),
        },
    )
    .unwrap();
    Connections::test(secrets, Arc::new(RefreshResponse(response)))
}
#[test]
fn terminal_refresh_errors_remove_only_invalid_credentials_and_require_reconnection() {
    for (provider, error) in [
        (Provider::Github, "bad_refresh_token"),
        (Provider::Github, "invalid_grant"),
        (Provider::Vk, "invalid_grant"),
    ] {
        let hub = expired_connection(
            provider,
            Ok(
                serde_json::json!({"error":error,"error_description":"do not display provider text"}),
            ),
        );
        assert!(hub.token(provider).is_err());
        assert!(hub
            .secrets()
            .get(&format!("{}-token", provider.key()))
            .unwrap()
            .is_none());
        assert_eq!(hub.status(provider).unwrap().status, "disconnected");
        assert!(hub.config(provider).is_ok());
    }
}
#[test]
fn transient_refresh_failures_preserve_credentials_for_retry() {
    for response in [
        Err("network timeout".into()),
        Ok(serde_json::json!({"error":"temporarily_unavailable"})),
        Ok(serde_json::json!({"error":"server_error"})),
        Ok(serde_json::json!({"error":"invalid_client"})),
    ] {
        let hub = expired_connection(Provider::Github, response);
        assert!(hub.token(Provider::Github).is_err());
        assert!(hub.secrets().get("github-token").unwrap().is_some());
    }
}
