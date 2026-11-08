//! Resolve only native-authorized originals; edges constrain partition rather
//! than assert authorship. Missing history never becomes fabricated context.
use super::{
    telegram_archive_json::TelegramChatHeader, telegram_document::TelegramDocument,
    telegram_identity::ArchiveVersion,
};
use crate::corpus_database::RevisionReference;
use std::collections::{BTreeMap, BTreeSet};
#[path = "telegram-relation-metadata.rs"]
pub(super) mod metadata;
#[derive(Debug)]
pub struct ResolvedRelations {
    pub related: Vec<RevisionReference>,
    pub complete: bool,
}
const LIMIT: &str = "Превышен бюджет проверки связей Telegram.";
pub fn resolve(
    version: ArchiveVersion,
    origin: &str,
    documents: &[(RevisionReference, TelegramDocument)],
) -> Result<Vec<ResolvedRelations>, String> {
    resolve_checked(version, origin, documents, &|| Ok(()))
}
pub fn resolve_checked(
    version: ArchiveVersion,
    origin: &str,
    documents: &[(RevisionReference, TelegramDocument)],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<ResolvedRelations>, String> {
    check()?;
    super::input::validate_origin(origin)?;
    if documents.len() > 100000
        || documents
            .iter()
            .map(|d| d.1.raw_json.len() + d.1.text.len())
            .sum::<usize>()
            > 128 * 1024 * 1024
    {
        return Err(LIMIT.into());
    }
    let metadata = documents
        .iter()
        .map(|d| {
            check()?;
            metadata::parse(&d.1)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_id: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut unique = BTreeSet::new();
    for (i, (reference, doc)) in documents.iter().enumerate() {
        check()?;
        if reference.document_id != doc.id
            || !unique.insert((&reference.document_id, &reference.revision_id))
        {
            return Err("Повторная или противоречивая ревизия Telegram.".into());
        }
        by_id.entry(&doc.id).or_default().push(i);
    }
    let mut links = vec![BTreeSet::new(); documents.len()];
    let mut complete: Vec<_> = metadata.iter().map(|m| m.complete).collect();
    let mut total = 0usize;
    for (i, (_, doc)) in documents.iter().enumerate() {
        check()?;
        if let Some(reply) = &doc.reply_to_original_id {
            let target = match metadata[i].peer.as_deref() {
                None => Some((doc.resource_id.clone(), typed_resource(doc))),
                Some(peer) => peer_resource(version, peer),
            };
            let mut found = false;
            if let Some((resource, typed)) = target {
                let (_, id) = version.identifiers(origin, &resource, reply)?;
                if let Some(candidates) = by_id.get(id.as_str()) {
                    for &other in candidates {
                        if other != i && typed_resource(&documents[other].1) == typed {
                            add(&mut links, &mut total, i, other)?;
                            found = true;
                        }
                    }
                    if candidates.len() > 1 {
                        complete[i] = false;
                    }
                }
            }
            complete[i] &= found;
        }
    }
    let mut quotes: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut quote_bytes = 0usize;
    for (i, item) in metadata.iter().enumerate() {
        check()?;
        for quote in &item.quotes {
            if !quotes.contains_key(quote.as_str()) {
                quote_bytes += quote.len();
                if quotes.len() >= 10000 || quote_bytes > 1024 * 1024 {
                    return Err(LIMIT.into());
                }
            }
            quotes.entry(quote).or_default().push(i);
        }
    }
    if !quotes.is_empty() {
        let quotes: Vec<_> = quotes.into_iter().collect();
        let patterns = quotes
            .iter()
            .map(|q| regex::escape(q.0))
            .collect::<Vec<_>>();
        let matcher = regex::RegexSetBuilder::new(patterns)
            .size_limit(16 * 1024 * 1024)
            .dfa_size_limit(8 * 1024 * 1024)
            .build()
            .map_err(|_| LIMIT)?;
        let mut attempts = 0usize;
        let mut found: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); quotes.len()];
        for (other, (_, doc)) in documents.iter().enumerate() {
            check()?;
            for pattern in matcher.matches(&doc.text).iter() {
                for &owner in &quotes[pattern].1 {
                    attempts += 1;
                    if attempts.is_multiple_of(1024) {
                        check()?;
                    }
                    if attempts > 1_000_000 {
                        return Err(LIMIT.into());
                    }
                    if owner != other {
                        add(&mut links, &mut total, owner, other)?;
                        found[pattern].insert(owner);
                    }
                }
            }
        }
        for (pattern, (_, owners)) in quotes.iter().enumerate() {
            for &owner in owners {
                complete[owner] &= found[pattern].contains(&owner);
            }
        }
    }
    Ok(links
        .into_iter()
        .zip(complete)
        .map(|(links, complete)| ResolvedRelations {
            related: links.into_iter().map(|i| documents[i].0.clone()).collect(),
            complete,
        })
        .collect())
}
fn add(
    links: &mut [BTreeSet<usize>],
    total: &mut usize,
    owner: usize,
    other: usize,
) -> Result<(), String> {
    if owner != other && links[owner].insert(other) {
        *total += 1;
        if *total > 1_000_000 || links[owner].len() > 128 {
            return Err(LIMIT.into());
        }
    }
    Ok(())
}
fn typed_resource(doc: &TelegramDocument) -> String {
    ArchiveVersion::V2.resource_id(&TelegramChatHeader {
        id: doc.resource_id.rsplit(':').next().unwrap_or("").into(),
        name: None,
        kind: doc.resource_kind.clone(),
    })
}
pub(super) fn peer_resource(version: ArchiveVersion, peer: &str) -> Option<(String, String)> {
    for class in ["user", "chat", "channel"] {
        if let Some(id) = peer.strip_prefix(class) {
            if id.len() > 128
                || !id
                    .as_bytes()
                    .first()
                    .is_some_and(|c| (b'1'..=b'9').contains(c))
                || !id.bytes().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            let typed = format!("telegram:peer:{class}:{id}");
            let resource = if version == ArchiveVersion::V1 {
                format!("telegram:chat:{id}")
            } else {
                typed.clone()
            };
            return Some((resource, typed));
        }
    }
    None
}
#[cfg(test)]
#[path = "telegram-episode-relations-tests.rs"]
mod tests;
