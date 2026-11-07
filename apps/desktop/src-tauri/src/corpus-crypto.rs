use crate::connections::storage::{sync_directory, SecretStore};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
const KEY_NAME: &str = "corpus-sqlcipher-key-v1";
const INVALID: &str =
    "Не удалось открыть зашифрованный корпус. Проверьте системное хранилище ключей.";
pub(super) fn key(path: &Path, secrets: &dyn SecretStore) -> Result<[u8; 32], String> {
    let stored = secrets.get(KEY_NAME)?;
    let mut key = [0; 32];
    if let Some(stored) = stored {
        let bytes = STANDARD.decode(stored).map_err(|_| INVALID)?;
        if bytes.len() != 32 {
            return Err(INVALID.into());
        }
        key.copy_from_slice(&bytes);
    } else {
        // Never invent a replacement key for an existing database, including a
        // damaged one. A missing key cannot turn into destructive reinitialization.
        if path.try_exists().map_err(|_| INVALID)? {
            return Err(INVALID.into());
        }
        rand::rngs::OsRng
            .try_fill_bytes(&mut key)
            .map_err(|_| INVALID)?;
        secrets.set(KEY_NAME, &STANDARD.encode(key))?;
    }
    Ok(key)
}
pub(super) fn open(path: &Path, key: &[u8; 32]) -> Result<Connection, String> {
    let parent = path.parent().ok_or(INVALID)?;
    std::fs::create_dir_all(parent).map_err(|_| INVALID)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(INVALID.into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(path)
                .map_err(|_| INVALID)?
                .sync_all()
                .map_err(|_| INVALID)?;
            sync_directory(parent)?;
        }
        Err(_) => return Err(INVALID.into()),
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| INVALID)?;
    let hex = key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    connection
        .pragma_update(None, "key", format!("x'{hex}'"))
        .map_err(|_| INVALID)?;
    let version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| INVALID)?;
    if !version.starts_with("4.") {
        return Err(INVALID.into());
    }
    connection.execute_batch("PRAGMA cipher_memory_security=ON; PRAGMA temp_store=MEMORY; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA trusted_schema=OFF; PRAGMA busy_timeout=5000;").map_err(|_|INVALID)?;
    let memory: i64 = connection
        .pragma_query_value(None, "temp_store", |row| row.get(0))
        .map_err(|_| INVALID)?;
    if memory != 2 {
        return Err(INVALID.into());
    }
    Ok(connection)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::storage::MemorySecrets;
    fn path() -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("proteus-cipher-native-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("metadata.db")
    }
    #[test]
    fn actual_cipher_encrypts_database_wal_and_checkpoint_backup_with_memory_temp() {
        let path = path();
        let secrets = MemorySecrets::default();
        let key = key(&path, &secrets).unwrap();
        let connection = open(&path, &key).unwrap();
        let version: String = connection
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .unwrap();
        let provider: String = connection
            .query_row("PRAGMA cipher_provider", [], |row| row.get(0))
            .unwrap();
        assert!(version.starts_with("4."));
        assert_eq!(provider, "openssl");
        connection
            .execute_batch(
                "CREATE TABLE exact(value TEXT NOT NULL); CREATE TEMP TABLE scratch(value TEXT);",
            )
            .unwrap();
        let text = "PROTEUS cipher fixture exact é 😀\r\nhttps://example.test/a-b?x_y=1";
        connection
            .execute("INSERT INTO exact VALUES(?1)", [text])
            .unwrap();
        connection
            .execute("INSERT INTO scratch VALUES(?1)", [text])
            .unwrap();
        assert!(path.with_file_name("metadata.db-wal").exists());
        assert_eq!(
            connection
                .query_row("PRAGMA temp_store", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        for entry in std::fs::read_dir(path.parent().unwrap()).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!bytes
                .windows(text.len())
                .any(|bytes| bytes == text.as_bytes()));
        }
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let backup = path.with_file_name("verified-backup.db");
        std::fs::copy(&path, &backup).unwrap();
        drop(connection);
        let restored = open(&backup, &key).unwrap();
        assert_eq!(
            restored
                .query_row("SELECT value FROM exact", [], |row| row.get::<_, String>(0))
                .unwrap(),
            text
        );
        drop(restored);
        assert!(open(&path, &[17; 32]).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn lost_or_invalid_os_key_never_replaces_existing_database() {
        let path = path();
        let secrets = MemorySecrets::default();
        let value = key(&path, &secrets).unwrap();
        let connection = open(&path, &value).unwrap();
        connection
            .execute_batch("CREATE TABLE exact(value TEXT)")
            .unwrap();
        drop(connection);
        let original = std::fs::read(&path).unwrap();
        secrets.remove(KEY_NAME).unwrap();
        assert!(key(&path, &secrets).is_err());
        assert!(secrets.get(KEY_NAME).unwrap().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        secrets.set(KEY_NAME, "invalid").unwrap();
        assert!(key(&path, &secrets).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
