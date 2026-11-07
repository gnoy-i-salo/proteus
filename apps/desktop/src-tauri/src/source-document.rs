use super::{
    grant::{iso, timestamp},
    model::{RawItem, SourceDocument, SourceGrantRequest},
};
use crate::connections::Provider;
use chrono::Utc;
use sha2::{Digest, Sha256};
pub(super) fn in_scope(request: &SourceGrantRequest, item: &RawItem) -> bool {
    if request
        .author_id
        .as_ref()
        .is_some_and(|author| item.author.as_ref() != Some(author))
    {
        return false;
    }
    if request.from.is_some() || request.to.is_some() {
        let Some(created) = item
            .created
            .as_deref()
            .and_then(|date| timestamp(date).ok())
        else {
            return false;
        };
        if request
            .from
            .as_deref()
            .and_then(|v| timestamp(v).ok())
            .is_some_and(|from| created < from)
            || request
                .to
                .as_deref()
                .and_then(|v| timestamp(v).ok())
                .is_some_and(|to| created > to)
        {
            return false;
        }
    }
    true
}
pub(super) fn document(
    request: &SourceGrantRequest,
    item: RawItem,
    retrieved: bool,
) -> SourceDocument {
    let content_hash = format!("{:x}", Sha256::digest(item.text.as_bytes()));
    let id = opaque_id(request, &item.id);
    let authorship = match (&request.author_id, &item.author) {
        (Some(expected), Some(actual)) if expected == actual => "owner",
        (_, Some(_)) => "other",
        _ => "unknown",
    };
    let partition = if retrieved
        && request.provider == Provider::Github
        && item
            .thread
            .as_ref()
            .is_some_and(|thread| Sha256::digest(thread.as_bytes())[0] % 5 == 0)
    {
        "holdout"
    } else {
        "reference"
    };
    SourceDocument {
        id,
        provider: request.provider,
        resource_id: request.resource_id.clone(),
        author_id: item.author,
        author_label: item.author_label,
        authorship: authorship.into(),
        kind: if item.kind.is_empty() {
            "document".into()
        } else {
            item.kind
        },
        mode: if item.mode.is_empty() {
            "formal".into()
        } else {
            item.mode
        },
        text: item.text,
        source_url: item.url,
        created_at: item
            .created
            .as_deref()
            .and_then(|date| timestamp(date).ok())
            .and_then(iso),
        retrieved_at: if retrieved {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        },
        content_hash,
        reply_to: item.reply_to.map(|parent| opaque_id(request, &parent)),
        is_forwarded: item.forwarded,
        truncated: false,
        partition: partition.into(),
    }
}
fn opaque_id(request: &SourceGrantRequest, raw: &str) -> String {
    format!(
        "d{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}",
                request.provider.key(),
                request.resource_id,
                raw
            )
            .as_bytes()
        )
    )
}
pub(super) fn bounded_document(mut doc: SourceDocument) -> SourceDocument {
    if doc.text.len() > 24000 {
        let mut end = 24000;
        while !doc.text.is_char_boundary(end) {
            end -= 1;
        }
        doc.text.truncate(end);
        doc.truncated = true;
    }
    doc
}
