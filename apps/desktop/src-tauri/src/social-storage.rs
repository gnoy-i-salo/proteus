use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{de::DeserializeOwned, Serialize};
use std::{fs, path::Path, sync::Arc};

pub(crate) trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn remove(&self, key: &str) -> Result<(), String>;
}

pub(crate) struct OsSecrets;
impl SecretStore for OsSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(
                "Системное хранилище секретов недоступно. Разблокируйте его и повторите.".into(),
            ),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        entry(key)?
            .set_password(value)
            .map_err(|_| "Не удалось сохранить секрет в системном хранилище.".into())
    }
    fn remove(&self, key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Не удалось удалить секрет из системного хранилища.".into()),
        }
    }
}
fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("dev.proteus-int.social", key)
        .map_err(|_| "Системное хранилище секретов недоступно.".into())
}
pub(crate) fn random_id(bytes: usize) -> Result<String, String> {
    let mut data = vec![0; bytes];
    rand::rngs::OsRng
        .try_fill_bytes(&mut data)
        .map_err(|_| "Генератор случайных чисел недоступен.".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(data))
}
pub(crate) fn load_secret<T: DeserializeOwned>(
    store: &dyn SecretStore,
    key: &str,
) -> Result<Option<T>, String> {
    store
        .get(key)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|_| {
                "Сохранённые данные подключения повреждены. Подключите аккаунт заново.".into()
            })
        })
        .transpose()
}
pub(crate) fn save_secret<T: Serialize>(
    store: &dyn SecretStore,
    key: &str,
    value: &T,
) -> Result<(), String> {
    let value = serde_json::to_string(value)
        .map_err(|_| "Не удалось подготовить данные подключения.".to_string())?;
    store.set(key, &value)
}

pub(crate) struct EncryptedStore {
    secrets: Arc<dyn SecretStore>,
}
impl EncryptedStore {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }
    pub fn write<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let plain = serde_json::to_vec(value)
            .map_err(|_| "Не удалось сериализовать корпус.".to_string())?;
        if plain.len() > 32 * 1024 * 1024 {
            return Err("Локальный корпус превысил лимит 32 МиБ.".into());
        }
        let bytes = self.encrypt(&plain)?;
        let parent = path.parent().ok_or("Неверный путь корпуса.")?;
        fs::create_dir_all(parent)
            .map_err(|_| "Не удалось создать каталог корпуса.".to_string())?;
        let temp = parent.join(format!("{}.tmp", random_id(16)?));
        let result = (|| {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temp)
                .map_err(|_| "Не удалось создать файл корпуса.".to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Не удалось записать корпус.".to_string())?;
            fs::rename(&temp, path).map_err(|_| "Не удалось сохранить корпус.".to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn read<T: DeserializeOwned>(&self, path: &Path) -> Result<T, String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "Корпус не найден.".to_string())?;
        if !metadata.is_file() || metadata.len() > 33 * 1024 * 1024 {
            return Err("Некорректный файл корпуса.".into());
        }
        let bytes = fs::read(path).map_err(|_| "Не удалось прочитать корпус.".to_string())?;
        let plain = self.decrypt(&bytes)?;
        serde_json::from_slice(&plain).map_err(|_| "Формат корпуса повреждён.".into())
    }
    fn key(&self) -> Result<Vec<u8>, String> {
        let encoded = match self.secrets.get("corpus-key-v1")? {
            Some(key) => key,
            None => {
                let key = random_id(32)?;
                self.secrets.set("corpus-key-v1", &key)?;
                key
            }
        };
        let key = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| "Ключ корпуса повреждён.".to_string())?;
        if key.len() != 32 {
            return Err("Ключ корпуса повреждён.".into());
        }
        Ok(key)
    }
    pub(crate) fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit, Payload},
            ChaCha20Poly1305,
        };
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key()?)
            .map_err(|_| "Ключ корпуса повреждён.".to_string())?;
        let mut nonce = [0; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| "Генератор случайных чисел недоступен.".to_string())?;
        let encrypted = cipher
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: plain,
                    aad: b"PROTEUS-CORPUS-V1",
                },
            )
            .map_err(|_| "Не удалось зашифровать корпус.".to_string())?;
        let mut result = b"PRT1".to_vec();
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&encrypted);
        Ok(result)
    }
    pub(crate) fn decrypt(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit, Payload},
            ChaCha20Poly1305, Nonce,
        };
        if bytes.len() < 32 || &bytes[..4] != b"PRT1" {
            return Err("Неподдерживаемый формат зашифрованного корпуса.".into());
        }
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key()?)
            .map_err(|_| "Ключ корпуса повреждён.".to_string())?;
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[4..16]),
                Payload {
                    msg: &bytes[16..],
                    aad: b"PROTEUS-CORPUS-V1",
                },
            )
            .map_err(|_| "Корпус повреждён или его ключ недоступен.".into())
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemorySecrets(pub std::sync::Mutex<std::collections::HashMap<String, String>>);
#[cfg(test)]
impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn remove(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corpus_round_trip_is_encrypted_and_rejects_tampering() {
        let secrets = Arc::new(MemorySecrets::default());
        let store = EncryptedStore::new(secrets.clone());
        let plaintext = b"private selected conversation";
        let mut encrypted = store.encrypt(plaintext).unwrap();
        assert!(!encrypted
            .windows(plaintext.len())
            .any(|window| window == plaintext));
        assert_eq!(store.decrypt(&encrypted).unwrap(), plaintext);
        assert_ne!(encrypted, store.encrypt(plaintext).unwrap());
        *encrypted.last_mut().unwrap() ^= 1;
        assert!(store.decrypt(&encrypted).is_err());
        assert!(EncryptedStore::new(Arc::new(MemorySecrets::default()))
            .decrypt(&encrypted)
            .is_err());
    }
}
