//! Native-authorized source projection. No frontend-authored episode assignments.
//! Source I/O is guarded; relation analysis runs outside the shared database gate.
use super::{
    grants::preview_deadline,
    operations::Operation,
    period::date_ms,
    preview_store::read_preview,
    telegram_document::decode_revision,
    telegram_identity::ArchiveVersion,
    telegram_relations::resolve_checked,
    telegram_worker::{completed_origin, selected},
};
use crate::{
    corpus_database::{episodes::EpisodeDocument, Snapshot},
    sources::Sources,
};
pub struct Projection {
    pub source: Snapshot,
    pub documents: Vec<EpisodeDocument>,
}
const LIMIT: &str = "Превышен бюджет подготовки исходных сообщений.";
pub fn project(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    now_ms: impl Fn() -> i64,
) -> Result<Projection, String> {
    project_checked(sources, operation, job_id, now_ms, &|| Ok(()))
}
pub(super) fn project_checked(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    now_ms: impl Fn() -> i64,
    check_extra: &dyn Fn() -> Result<(), String>,
) -> Result<Projection, String> {
    check_extra()?;
    let started = now_ms();
    let (job, version, origin, source, originals, deadline) =
        sources.with_ingestion_database(operation, |db| {
            let job = super::job_workflow::read_job(db, job_id)?;
            // Resolve current permissions before snapshot() reads any originals.
            super::job_workflow::validate_selection(db, &job.state.selection, started)?;
            let preview = read_preview(db, &job.state.selection.preview_id)?;
            let version = ArchiveVersion::from_adapter(&job.state.checkpoint.adapter_version)?;
            if preview.adapter_version != version.adapter() {
                return Err("Версии задачи и предпросмотра не совпадают.".into());
            }
            let origin = completed_origin(&job, &preview.fingerprint, started)?;
            let deadline = job
                .state
                .selection
                .bindings
                .iter()
                .map(|binding| preview_deadline(db, binding, &preview.id, started))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .min()
                .ok_or("Нет разрешённых привязок.")?;
            let check = || {
                check_extra()?;
                access(operation, started, deadline, now_ms())
            };
            let source = db.snapshot_checked(
                job.state
                    .published_snapshot_id
                    .as_deref()
                    .ok_or("Нет исходного снимка.")?,
                &check,
            )?;
            if source.corpus_id != format!("corpus:{job_id}")
                || source.publication_id != format!("import:{job_id}")
                || source.documents.len() > 100000
            {
                return Err(LIMIT.into());
            }
            let mut originals = Vec::with_capacity(source.documents.len());
            let mut bytes = 0usize;
            for reference in &source.documents {
                check()?;
                let original = db.revision(reference)?;
                bytes += original.document_json.len();
                if bytes > 128 * 1024 * 1024 {
                    return Err(LIMIT.into());
                }
                originals.push(original);
            }
            check()?;
            Ok((job, version, origin, source, originals, deadline))
        })?;
    let check = || {
        check_extra()?;
        access(operation, started, deadline, now_ms())
    };
    let mut decoded = Vec::with_capacity(originals.len());
    for original in originals {
        check()?;
        decoded.push((
            original.reference.clone(),
            decode_revision(version, &origin, &original)?,
        ));
    }
    let relations = resolve_checked(version, &origin, &decoded, &check)?;
    let mut documents = Vec::new();
    let mut bytes = 0usize;
    let mut edges = 0usize;
    for ((reference, doc), relation) in decoded.into_iter().zip(relations) {
        check()?;
        let mut assigned = false;
        for binding in &job.state.selection.bindings {
            if !selected(std::slice::from_ref(binding), &doc)? {
                continue;
            }
            bytes += doc.text.len();
            edges += relation.related.len();
            if documents.len() >= 100000 || bytes > 128 * 1024 * 1024 || edges > 1_000_000 {
                return Err(LIMIT.into());
            }
            documents.push(EpisodeDocument {
                reference: reference.clone(),
                source_key: doc.source_key.clone(),
                binding_id: binding.id.clone(),
                conversation_id: conversation_key(&origin, &doc.resource_id)?,
                thread_id: None,
                timestamp_ms: doc.created_at.as_deref().map(date_ms).transpose()?,
                text: doc.text.clone(),
                related: relation.related.clone(),
                relations_complete: relation.complete,
                holdout: false,
            });
            assigned = true;
        }
        if !assigned {
            return Err("Ревизия не входит в разрешённую область задачи.".into());
        }
    }
    // A completed receipt and an earlier successful grant check are not access
    // tokens. Deletion/revocation while analyzing must win before returning data.
    sources.with_ingestion_database(operation, |db| {
        check()?;
        let current = super::job_workflow::read_job(db, job_id)?;
        if current != job {
            return Err("Исходная задача изменилась.".into());
        }
        super::job_workflow::validate_selection(db, &current.state.selection, now_ms())?;
        if db.snapshot_checked(&source.id, &check)? != source {
            return Err("Исходный снимок изменился.".into());
        }
        check()
    })?;
    Ok(Projection { source, documents })
}
pub(super) fn access(
    operation: &Operation,
    started: i64,
    deadline: i64,
    now: i64,
) -> Result<(), String> {
    operation.check()?;
    if now < started || now >= deadline {
        return Err("Разрешение источника истекло во время подготовки.".into());
    }
    Ok(())
}

fn conversation_key(origin: &str, resource: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(origin, resource))
        .map_err(|_| "Не удалось определить диалог источника.")?;
    Ok(format!(
        "telegramConversation:1:{:x}",
        Sha256::digest(bytes)
    ))
}
