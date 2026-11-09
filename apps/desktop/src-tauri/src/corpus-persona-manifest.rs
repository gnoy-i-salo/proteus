//! Immutable persona manifests and their exact skill bundle. Storage validation
//! is independent of live grant, profile exemplar and model-lease authority.
use super::*;
use crate::memory::types::{
    decode_persona_manifest, decode_skill_definition, MemoryItemKind, MemoryItemSupport,
    PersonaManifest, SkillDefinition, SkillSupport,
};
use std::collections::HashMap;

const MAX_MANIFEST_BYTES: i64 = 8 * 1024 * 1024;
const SAVEPOINT: &str = "proteus_persona_manifest_read";
const MISSING: &str = "Манифест персоны не найден или удалён.";
const CONFLICT: &str = "Манифест уже сохранён с другими данными.";

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestBundle {
    pub manifest: PersonaManifest,
    pub skills: Vec<SkillDefinition>,
}
impl CorpusDatabase {
    pub fn freeze_manifest_checked(
        &mut self,
        manifest: &PersonaManifest,
        skills: &[SkillDefinition],
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<ManifestBundle, String> {
        check()?;
        validate_shape(manifest, skills)?;
        let json = bounded_json(manifest, MAX_MANIFEST_BYTES as usize, BUNDLE_BUDGET)?;
        let mut total = json.len();
        let mut skill_json = Vec::with_capacity(skills.len());
        for skill in skills {
            check()?;
            let value = bounded_json(skill, MAX_ITEM_PAYLOAD_BYTES as usize, ITEM_BUDGET)?;
            total += value.len();
            if total > MAX_BUNDLE_PAYLOAD_BYTES as usize {
                return Err(BUNDLE_BUDGET.into());
            }
            skill_json.push(value);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        check()?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_manifests WHERE id=?1)",
                [&manifest.id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if !exists {
            validate_bindings(&tx, manifest, skills, check)?;
            tx.execute("INSERT INTO memory_manifests(id,corpus_snapshot_id,memory_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![manifest.id,manifest.corpus_snapshot_id,manifest.memory_snapshot_id,manifest.persona_id,json.len() as i64,json,digest(json.as_bytes())]).map_err(sql_error)?;
            for (skill, value) in skills.iter().zip(&skill_json) {
                check()?;
                tx.execute("INSERT INTO memory_skills(id,manifest_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5)", params![skill.id,manifest.id,value.len() as i64,value,digest(value.as_bytes())]).map_err(sql_error)?;
            }
        }
        let stored = load(&tx, &manifest.id, &manifest.persona_id, check)?;
        if stored.manifest != *manifest || stored.skills != skills {
            return Err(CONFLICT.into());
        }
        check()?;
        tx.commit().map_err(sql_error)?;
        // No fallible callback after a successful commit.
        Ok(stored)
    }

    pub fn read_manifest_checked(
        &self,
        id: &str,
        persona_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<ManifestBundle, String> {
        check()?;
        validate_persona_id(id)?;
        validate_persona_id(persona_id)?;
        self.connection
            .execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))
            .map_err(sql_error)?;
        match load(&self.connection, id, persona_id, check) {
            Ok(value) => {
                self.connection
                    .execute_batch(&format!("RELEASE {SAVEPOINT}"))
                    .map_err(sql_error)?;
                check()?;
                Ok(value)
            }
            Err(error) => {
                let _ = self
                    .connection
                    .execute_batch(&format!("ROLLBACK TO {SAVEPOINT}; RELEASE {SAVEPOINT}"));
                Err(error)
            }
        }
    }
}

fn validate_shape(manifest: &PersonaManifest, skills: &[SkillDefinition]) -> Result<(), String> {
    manifest.validate()?;
    if manifest.skill_ids.len() != skills.len() {
        return Err(INVALID.into());
    }
    for (id, skill) in manifest.skill_ids.iter().zip(skills) {
        skill.validate()?;
        if *id != skill.id {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

fn validate_bindings(
    connection: &Connection,
    manifest: &PersonaManifest,
    skills: &[SkillDefinition],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let memory = load_bundle(
        connection,
        &manifest.memory_snapshot_id,
        &manifest.persona_id,
        check,
    )?;
    if memory.snapshot.corpus_snapshot_id != manifest.corpus_snapshot_id {
        return Err(INVALID.into());
    }
    let source = read_snapshot_checked(connection, &manifest.corpus_snapshot_id, check)?;
    let source_pairs: HashSet<_> = source
        .documents
        .iter()
        .map(|r| (&r.document_id, &r.revision_id))
        .collect();
    let pinned_pairs: HashSet<_> = manifest
        .source_refs
        .iter()
        .map(|r| (&r.document_id, &r.revision_id))
        .collect();
    if !pinned_pairs.is_subset(&source_pairs) {
        return Err(INVALID.into());
    }
    let by_id: HashMap<_, _> = memory.items.iter().map(|item| (&item.id, item)).collect();
    for skill in skills {
        check()?;
        let referenced: Vec<_> = skill
            .memory_item_ids
            .iter()
            .map(|id| by_id.get(id).copied().ok_or(INVALID))
            .collect::<Result<_, _>>()?;
        if skill.support == SkillSupport::Observed
            && referenced.iter().any(|item| {
                item.kind != MemoryItemKind::Procedure
                    || item.support != MemoryItemSupport::Observed
                    || !item.contradiction_ids.is_empty()
                    || item.source_spans.is_empty()
            })
        {
            return Err("Наблюдаемый навык требует непротиворечивых наблюдаемых процедур.".into());
        }
        if skill.source_spans.is_empty() {
            return Err(INVALID.into());
        }
        for span in &skill.source_spans {
            if !pinned_pairs.contains(&(&span.document_id, &span.revision_id))
                || !referenced.iter().any(|item| {
                    item.source_spans.iter().any(|host| {
                        host.document_id == span.document_id
                            && host.revision_id == span.revision_id
                            && host.unit == span.unit
                            && host.start <= span.start
                            && span.end <= host.end
                    })
                })
            {
                return Err(SPAN_OUTSIDE_SOURCE.into());
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Artifact {
    Manifest,
    Skill,
}
impl Artifact {
    fn table(self) -> &'static str {
        match self {
            Self::Manifest => "memory_manifests",
            Self::Skill => "memory_skills",
        }
    }
    fn cap(self) -> i64 {
        match self {
            Self::Manifest => MAX_MANIFEST_BYTES,
            Self::Skill => MAX_ITEM_PAYLOAD_BYTES,
        }
    }
}

/// Validate counters before materializing payloads, including corrupt rows
/// written outside this API. Table names come only from the closed enum.
fn read_json(connection: &Connection, kind: Artifact, id: &str) -> Result<String, String> {
    let table = kind.table();
    let (bytes, actual): (i64, i64) = connection
        .query_row(
            &format!(
                "SELECT payload_bytes,length(CAST(data_json AS BLOB)) FROM {table} WHERE id=?1"
            ),
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or(MISSING)?;
    if bytes < 1 || bytes > kind.cap() || bytes != actual {
        return Err(INVALID.into());
    }
    let (json, hash): (String, String) = connection
        .query_row(
            &format!("SELECT data_json,canonical_sha256 FROM {table} WHERE id=?1"),
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql_error)?;
    if json.len() as i64 != bytes || digest(json.as_bytes()) != hash {
        return Err(INVALID.into());
    }
    Ok(json)
}

fn load(
    connection: &Connection,
    id: &str,
    persona_id: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ManifestBundle, String> {
    check()?;
    let (owner, corpus, memory): (String, String, String) = connection.query_row("SELECT persona_id,corpus_snapshot_id,memory_snapshot_id FROM memory_manifests WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(sql_error)?.ok_or(MISSING)?;
    if owner != persona_id {
        return Err(FOREIGN_OWNER.into());
    }
    let json = read_json(connection, Artifact::Manifest, id)?;
    let manifest = decode_persona_manifest(&json)?;
    if manifest.id != id
        || manifest.persona_id != persona_id
        || manifest.corpus_snapshot_id != corpus
        || manifest.memory_snapshot_id != memory
    {
        return Err(INVALID.into());
    }
    // Sum/count/individual limits are verified before any skill JSON allocation.
    let (count, total, invalid): (i64, i64, i64) = connection.query_row("SELECT count(*),coalesce(sum(payload_bytes),0),coalesce(sum(CASE WHEN payload_bytes<1 OR payload_bytes>524288 OR payload_bytes!=length(CAST(data_json AS BLOB)) THEN 1 ELSE 0 END),0) FROM memory_skills WHERE manifest_id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(sql_error)?;
    if count != manifest.skill_ids.len() as i64
        || invalid != 0
        || !(0..=MAX_BUNDLE_PAYLOAD_BYTES - json.len() as i64).contains(&total)
    {
        return Err(INVALID.into());
    }
    let mut skills = Vec::with_capacity(manifest.skill_ids.len());
    for skill_id in &manifest.skill_ids {
        check()?;
        let belongs: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_skills WHERE id=?1 AND manifest_id=?2)",
                params![skill_id, id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if !belongs {
            return Err(INVALID.into());
        }
        skills.push(decode_skill_definition(&read_json(
            connection,
            Artifact::Skill,
            skill_id,
        )?)?);
    }
    validate_shape(&manifest, &skills)?;
    validate_bindings(connection, &manifest, &skills, check)?;
    check()?;
    Ok(ManifestBundle { manifest, skills })
}
