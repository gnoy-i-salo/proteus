//! Host-owned binding actions. Callers select an opaque ID, never supply authority.
use super::{
    binding_policy::{
        self, CreatedSourceBinding, DocumentRelation, ModelSourceDocument, RelationKind,
        RevisionReference, SourceAuthor, SourceBindingRequest,
    },
    corpus::Sources,
    document::bounded_document,
    model::{Record, SourceAction, SourceCoverage, SourceDocument},
};
use chrono::Utc;
use serde::Serialize;
use std::sync::atomic::Ordering;
#[cfg_attr(test, derive(serde::Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBindingResult {
    pub snapshot_id: String,
    pub documents: Vec<ModelSourceDocument>,
    pub coverage: SourceCoverage,
    pub cursor: Option<String>,
    pub limitations: Vec<String>,
}
fn relations(document: &SourceDocument, record: &Record) -> Result<Vec<DocumentRelation>, String> {
    let mut relations = Vec::new();
    if let Some(reply) = &document.reply_to {
        // A reference can name an original only when that original is in the same
        // native-approved reference selection. Display labels never resolve identity.
        let original = record
            .documents
            .iter()
            .find(|candidate| candidate.id == *reply && candidate.partition == "reference");
        relations.push(DocumentRelation {
            kind: RelationKind::Reply,
            original: original
                .map(|doc| {
                    Ok::<_, String>(RevisionReference {
                        document_id: doc.id.clone(),
                        revision_id: binding_policy::revision_id(doc)?,
                    })
                })
                .transpose()?,
            original_author: SourceAuthor {
                provider: original.map(|doc| doc.provider),
                id: original.and_then(|doc| doc.author_id.clone()),
                label: original.and_then(|doc| doc.author_label.clone()),
            },
            span: None,
        });
    }
    if document.is_forwarded {
        relations.push(DocumentRelation {
            kind: RelationKind::Forward,
            original: None,
            original_author: SourceAuthor {
                provider: None,
                id: None,
                label: None,
            },
            span: None,
        });
    }
    Ok(relations)
}
impl Sources {
    pub(super) fn create_binding(
        &self,
        request: SourceBindingRequest,
    ) -> Result<CreatedSourceBinding, String> {
        let record = self.create_record(request.source.clone(), Some(request))?;
        Ok(CreatedSourceBinding {
            binding: record
                .binding
                .ok_or("Отсутствует выбранная область источника.")?,
            grant: record.grant,
        })
    }
    pub(super) fn binding_action(
        &self,
        binding_id: &str,
        action: SourceAction,
    ) -> Result<SourceBindingResult, String> {
        let id = binding_id
            .strip_prefix("binding:")
            .ok_or("Неверная привязка источника.")?;
        let path = self.path(id)?;
        let result = self.execute_scoped(id, Some(binding_id), action)?;
        // Collection can perform I/O. Reload persisted authority after it, before any
        // model projection, so cancellation/revocation/account changes fail closed.
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let record: Record = self.0.store.read(&path)?;
        let binding = record
            .binding
            .as_ref()
            .filter(|binding| binding.id == binding_id)
            .ok_or("Неверная привязка источника.")?;
        let now = Utc::now().timestamp_millis();
        binding_policy::validate_authority(binding, &record.grant, now)?;
        if record.cancelled
            || self.flag(id)?.load(Ordering::SeqCst)
            || result.snapshot_id != record.grant.snapshot_id
        {
            return Err("Привязка источника больше не доступна.".into());
        }
        if binding.resource_id.starts_with("import:") {
            if self.imported(&binding.resource_id)?.provider != binding.provider {
                return Err("Импорт принадлежит другому сервису.".into());
            }
        } else if self.0.connections.account_id(binding.provider)? != record.grant.account_id {
            return Err("Подключённый аккаунт изменился. Выберите источник заново.".into());
        }
        let mut documents = Vec::new();
        for document in result.documents {
            let original = record
                .documents
                .iter()
                .find(|original| {
                    original.id == document.id && original.content_hash == document.content_hash
                })
                .ok_or("Версия документа больше не доступна.")?;
            let mut projected = binding_policy::project(
                binding,
                &record.grant,
                &bounded_document(original.clone()),
                relations(original, &record)?,
                now,
            )?;
            // Model text can be a bounded prefix, but revision identity always names
            // the complete stored source document, including its original metadata.
            projected.revision_id = binding_policy::revision_id(original)?;
            documents.push(projected);
        }
        Ok(SourceBindingResult {
            snapshot_id: result.snapshot_id,
            documents,
            coverage: result.coverage,
            cursor: result.cursor,
            limitations: result.limitations,
        })
    }
}

#[cfg(test)]
#[path = "source-binding-tests.rs"]
mod tests;
