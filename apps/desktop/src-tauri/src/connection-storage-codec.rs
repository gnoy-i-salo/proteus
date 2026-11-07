//! Native-only persistence. Legacy spellings never become accepted IPC aliases.
use super::{
    model::{ConnectionConfig, DevicePending, Provider, Token},
    oauth::VkPending,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

const FORMAT: &str = "proteus-connection-secret";
const VERSION: u8 = 2;
const MAX_BYTES: usize = 1024 * 1024;
const INVALID: &str = "Сохранённые данные подключения повреждены. Подключите аккаунт заново.";

pub(crate) trait StoredSecret: Serialize + DeserializeOwned {
    const KIND: &'static str;
    fn decode_legacy(value: Value) -> Result<Self, String>;
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    format: String,
    version: u8,
    kind: String,
    payload: T,
}
pub(super) fn decode<T: StoredSecret>(data: &str) -> Result<T, String> {
    if data.len() > MAX_BYTES {
        return Err(INVALID.into());
    }
    let value: Value = serde_json::from_str(data).map_err(|_| INVALID)?;
    if value.get("format").is_some() || value.get("version").is_some() {
        let envelope: Envelope<T> = serde_json::from_value(value).map_err(|_| INVALID)?;
        if envelope.format != FORMAT || envelope.version != VERSION || envelope.kind != T::KIND {
            return Err(INVALID.into());
        }
        Ok(envelope.payload)
    } else {
        T::decode_legacy(value)
    }
}
pub(super) fn encode<T: StoredSecret>(value: &T) -> Result<String, String> {
    let data = serde_json::to_string(&Envelope {
        format: FORMAT.into(),
        version: VERSION,
        kind: T::KIND.into(),
        payload: value,
    })
    .map_err(|_| INVALID)?;
    // Validate the full generation before replacing the same Keychain item.
    let _: T = decode(&data)?;
    Ok(data)
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyConfig {
    provider: Option<Provider>,
    client_id: Option<String>,
    api_id: Option<i32>,
    api_hash: Option<String>,
    token: Option<String>,
}
impl StoredSecret for ConnectionConfig {
    const KIND: &'static str = "config";
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let old: LegacyConfig = serde_json::from_value(value).map_err(|_| INVALID)?;
        Ok(Self {
            provider: old.provider,
            client_id: old.client_id,
            api_id: old.api_id,
            api_hash: old.api_hash,
            token: old.token,
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyToken {
    access: String,
    refresh: Option<String>,
    device_id: Option<String>,
    expires_at: Option<i64>,
    refreshed_at: i64,
    account_id: String,
    label: String,
}
impl StoredSecret for Token {
    const KIND: &'static str = "token";
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let old: LegacyToken = serde_json::from_value(value).map_err(|_| INVALID)?;
        Ok(Self {
            access: old.access,
            refresh: old.refresh,
            device_id: old.device_id,
            expires_at: old.expires_at,
            refreshed_at: old.refreshed_at,
            account_id: old.account_id,
            label: old.label,
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyDevicePending {
    client_id: String,
    device_code: String,
    user_code: String,
    expires_at: i64,
    next_poll: i64,
    interval: i64,
}
impl StoredSecret for DevicePending {
    const KIND: &'static str = "githubPending";
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let old: LegacyDevicePending = serde_json::from_value(value).map_err(|_| INVALID)?;
        Ok(Self {
            client_id: old.client_id,
            device_code: old.device_code,
            user_code: old.user_code,
            expires_at: old.expires_at,
            next_poll: old.next_poll,
            interval: old.interval,
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyVkPending {
    state: String,
    verifier: String,
    client_id: String,
    expires_at: i64,
}
impl StoredSecret for VkPending {
    const KIND: &'static str = "vkPending";
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let old: LegacyVkPending = serde_json::from_value(value).map_err(|_| INVALID)?;
        Ok(Self {
            state: old.state,
            verifier: old.verifier,
            client_id: old.client_id,
            expires_at: old.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::storage::{load_secret, save_secret, SecretStore};
    use serde_json::json;
    use std::sync::Mutex;

    struct SyntheticSecret(Mutex<String>);
    impl SecretStore for SyntheticSecret {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Ok(Some(self.0.lock().unwrap().clone()))
        }
        fn set(&self, _: &str, value: &str) -> Result<(), String> {
            *self.0.lock().unwrap() = value.into();
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), String> {
            panic!("test must not delete a secret")
        }
    }
    fn legacy_token() -> serde_json::Value {
        json!({"access":"synthetic-only","refresh":null,"device-id":"device:1","expires-at":123,"refreshed-at":45,"account-id":"actor:1","label":"  Имя é😀\r\n"})
    }
    #[test]
    fn legacy_secret_read_preserves_bytes_without_migration_side_effects() {
        let original = format!("  {}\n", legacy_token());
        let store = SyntheticSecret(Mutex::new(original.clone()));
        let token = load_secret::<Token>(&store, "github-token")
            .unwrap()
            .unwrap();
        assert_eq!(token.label, "  Имя é😀\r\n");
        assert_eq!(token.expires_at, Some(123));
        assert_eq!(store.0.lock().unwrap().as_str(), original);
    }
    #[test]
    fn legacy_secret_rejects_mixed_aliases_even_when_identical() {
        let mut value = legacy_token();
        value["accountId"] = json!("actor:1");
        let store = SyntheticSecret(Mutex::new(value.to_string()));
        assert!(load_secret::<Token>(&store, "github-token").is_err());
    }
    #[test]
    fn legacy_secret_rejects_unknown_metadata() {
        let mut value = legacy_token();
        value["unknown"] = json!("must reject");
        let store = SyntheticSecret(Mutex::new(value.to_string()));
        assert!(load_secret::<Token>(&store, "github-token").is_err());
    }
    #[test]
    fn new_secret_write_is_versioned_and_canonical() {
        let store = SyntheticSecret(Mutex::new(legacy_token().to_string()));
        let token = load_secret::<Token>(&store, "github-token")
            .unwrap()
            .unwrap();
        save_secret(&store, "github-token", &token).unwrap();
        let value: serde_json::Value = serde_json::from_str(&store.0.lock().unwrap()).unwrap();
        assert_eq!(value["format"], "proteus-connection-secret");
        assert_eq!(value["version"], 2);
        assert_eq!(value["payload"]["accountId"], "actor:1");
        assert!(value["payload"].get("account-id").is_none());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::connections::storage::{load_secret, save_secret, MemorySecrets, SecretStore};
    use serde_json::json;
    #[test]
    fn all_four_secret_schemas_upgrade_without_losing_nulls_expiry_or_polling() {
        fn check<T: StoredSecret>(old: Value, expected: Value) {
            let decoded: T = decode(&old.to_string()).unwrap();
            let encoded = encode(&decoded).unwrap();
            let value: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(value["payload"], expected);
            assert_eq!(value["version"], 2);
            assert_eq!(value["kind"], T::KIND);
            let again: T = decode(&encoded).unwrap();
            assert_eq!(encode(&again).unwrap(), encoded);
        }
        check::<ConnectionConfig>(
            json!({"provider":"telegram","client-id":null,"api-id":123,"api-hash":"synthetic-only","token":null}),
            json!({"provider":"telegram","clientId":null,"apiId":123,"apiHash":"synthetic-only","token":null}),
        );
        check::<Token>(
            json!({"access":"synthetic-only","refresh":null,"device-id":null,"expires-at":null,"refreshed-at":77,"account-id":"a:1","label":"  é\r\n"}),
            json!({"access":"synthetic-only","refresh":null,"deviceId":null,"expiresAt":null,"refreshedAt":77,"accountId":"a:1","label":"  é\r\n"}),
        );
        check::<DevicePending>(
            json!({"client-id":"c","device-code":"synthetic-only","user-code":"u","expires-at":111,"next-poll":99,"interval":5}),
            json!({"clientId":"c","deviceCode":"synthetic-only","userCode":"u","expiresAt":111,"nextPoll":99,"interval":5}),
        );
        check::<VkPending>(
            json!({"state":"synthetic-state","verifier":"synthetic-verifier","client-id":"c","expires-at":101}),
            json!({"state":"synthetic-state","verifier":"synthetic-verifier","clientId":"c","expiresAt":101}),
        );
    }
    #[test]
    fn rejects_corrupt_unknown_versions_wrong_kinds_aliases_and_oversize_without_leaking() {
        let config = ConnectionConfig {
            client_id: Some("synthetic-private".into()),
            ..ConnectionConfig::default()
        };
        let valid: Value = serde_json::from_str(&encode(&config).unwrap()).unwrap();
        for (key, value) in [
            ("version", json!(99)),
            ("kind", json!("token")),
            ("format", json!("other")),
            ("unknown", json!(true)),
        ] {
            let mut corrupt = valid.clone();
            corrupt[key] = value;
            let error = decode::<ConnectionConfig>(&corrupt.to_string())
                .err()
                .unwrap();
            assert!(!error.contains("synthetic-private"));
        }
        let mut corrupt = valid;
        corrupt["payload"]["client-id"] = json!("synthetic-private");
        assert!(decode::<ConnectionConfig>(&corrupt.to_string()).is_err());
        assert!(decode::<ConnectionConfig>("{").is_err());
        assert!(decode::<ConnectionConfig>(&" ".repeat(MAX_BYTES + 1)).is_err());
    }
    struct FailedWrite(MemorySecrets);
    impl SecretStore for FailedWrite {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            self.0.get(key)
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("synthetic denied write".into())
        }
        fn remove(&self, _: &str) -> Result<(), String> {
            panic!("must not delete original")
        }
    }
    #[test]
    fn failed_keychain_write_keeps_legacy_record_without_backup_secret() {
        let store = FailedWrite(MemorySecrets::default());
        let old = " {\"client-id\":\"synthetic-client\"} \n";
        store.0.set("github-config", old).unwrap();
        let config: ConnectionConfig = load_secret(&store, "github-config").unwrap().unwrap();
        assert!(save_secret(&store, "github-config", &config).is_err());
        assert_eq!(store.get("github-config").unwrap().unwrap(), old);
        assert_eq!(store.0 .0.lock().unwrap().len(), 1);
    }
}
