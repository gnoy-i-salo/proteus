//! A verified cached bundle stays usable only while the immutable database
//! identity still exists. This is a lifetime check, not a replacement reader.
use super::{sql_error, INVALID};
use super::{CorpusDatabase, ManifestBundle};
use rusqlite::{params, Connection, OptionalExtension};

pub struct ManifestPin {
    bundle: ManifestBundle,
    manifest_identity: Identity,
    skill_identities: Vec<Identity>,
}
struct Identity {
    bytes: i64,
    sha256: String,
}
impl ManifestPin {
    pub fn bundle(&self) -> &ManifestBundle {
        &self.bundle
    }
}
impl CorpusDatabase {
    pub fn pin_manifest_checked(
        &self,
        id: &str,
        persona_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<ManifestPin, String> {
        check()?;
        // The outer savepoint pins the verified payload and its stored hashes
        // together. Preserve any caller transaction and original JSON spelling.
        self.connection
            .execute_batch("SAVEPOINT proteus_manifest_pin")
            .map_err(sql_error)?;
        let result = (|| {
            let bundle = self.read_manifest_checked(id, persona_id, check)?;
            let manifest_identity = identity(&self.connection, "memory_manifests", id)?;
            let mut skill_identities = Vec::with_capacity(bundle.skills.len());
            for skill in &bundle.skills {
                check()?;
                skill_identities.push(identity(&self.connection, "memory_skills", &skill.id)?);
            }
            let pin = ManifestPin {
                bundle,
                manifest_identity,
                skill_identities,
            };
            self.verify_manifest_pin(&pin, check)?;
            Ok(pin)
        })();
        match result {
            Ok(pin) => {
                self.connection
                    .execute_batch("RELEASE proteus_manifest_pin")
                    .map_err(sql_error)?;
                check()?;
                Ok(pin)
            }
            Err(error) => {
                let _ = self.connection.execute_batch(
                    "ROLLBACK TO proteus_manifest_pin; RELEASE proteus_manifest_pin",
                );
                Err(error)
            }
        }
    }
    pub fn verify_manifest_pin(
        &self,
        pin: &ManifestPin,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        check()?;
        let manifest = &pin.bundle.manifest;
        // No payload allocation on frequent session/CLI authority checks.
        // Every cached payload was fully validated before this pin was issued;
        // native immutable tables prohibit mutation and deletion invalidates it.
        let matches: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_manifests WHERE id=?1 AND persona_id=?2 AND corpus_snapshot_id=?3 AND memory_snapshot_id=?4 AND payload_bytes=?5 AND canonical_sha256=?6)",
            params![manifest.id, manifest.persona_id, manifest.corpus_snapshot_id, manifest.memory_snapshot_id, pin.manifest_identity.bytes, pin.manifest_identity.sha256],
            |row| row.get(0),
        ).map_err(sql_error)?;
        let count: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM memory_skills WHERE manifest_id=?1",
                [&manifest.id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if !matches || count != pin.bundle.skills.len() as i64 {
            return Err(INVALID.into());
        }
        for (skill, identity) in pin.bundle.skills.iter().zip(&pin.skill_identities) {
            check()?;
            let matches: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_skills WHERE id=?1 AND manifest_id=?2 AND payload_bytes=?3 AND canonical_sha256=?4)",
                params![skill.id, manifest.id, identity.bytes, identity.sha256], |row| row.get(0),
            ).map_err(sql_error)?;
            if !matches {
                return Err(INVALID.into());
            }
        }
        check()
    }
}

// Table is selected by this module's two literal callers, never caller input.
fn identity(connection: &Connection, table: &str, id: &str) -> Result<Identity, String> {
    let (bytes, hash): (i64, Option<String>) = connection.query_row(
        &format!("SELECT payload_bytes,CASE WHEN length(CAST(canonical_sha256 AS BLOB))=64 THEN canonical_sha256 END FROM {table} WHERE id=?1"),
        [id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(sql_error)?.ok_or(INVALID)?;
    let sha256 = hash.ok_or(INVALID)?;
    if bytes < 1
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(INVALID.into());
    }
    Ok(Identity { bytes, sha256 })
}
