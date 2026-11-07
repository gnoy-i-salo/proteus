//! Deterministic preparation over native-authorized immutable source revisions.
//! This is not an IPC or grant authority; callers must authorize before reading
//! originals and again before exposing any prepared reference content.
//!
//! Acquisition supplies account-scoped conversation keys and resolved reply,
//! quote, forward and overlap links. Unresolved/incomplete relations are kept
//! out of reference; this engine does not pretend to infer arbitrary unlabelled
//! partial quotations. Exact whole-text copies share a partition without losing
//! either original. A prepared snapshot and its actual tokenizer are integrated
//! by the native preparation workflow, not by a frontend-supplied assignment.
#[path = "corpus-episode-chunks.rs"]
mod chunks;
#[path = "corpus-episode-types.rs"]
mod types;
use super::types::digest;
use std::collections::{BTreeMap, BTreeSet, HashMap};
pub use types::{
    ChunkPolicy, ChunkSpan, EpisodeDocument, EpisodePolicy, EpisodeTokenizer, Partition,
    PreparedChunk, PreparedEpisode, PreparedEpisodes, SpanUnit,
};
const INVALID: &str = "Неверные данные или политика подготовки эпизодов.";

pub fn prepare<'a>(
    snapshot_id: &str,
    documents: &'a [EpisodeDocument],
    policy: &EpisodePolicy,
) -> Result<PreparedEpisodes<'a>, String> {
    validate(snapshot_id, documents, policy)?;
    let mut assembly = Groups::new(documents.len());
    let mut leakage = Groups::new(documents.len());
    let mut flags = vec![BTreeSet::new(); documents.len()];
    let refs: HashMap<_, _> = documents
        .iter()
        .enumerate()
        .map(|(i, d)| {
            (
                (
                    d.reference.document_id.as_str(),
                    d.reference.revision_id.as_str(),
                ),
                i,
            )
        })
        .collect();
    let mut threads = HashMap::new();
    let mut context_threads = HashMap::new();
    let mut sources = HashMap::new();
    let mut identities = HashMap::new();
    let mut copies = HashMap::new();
    let mut periods: BTreeMap<_, Vec<usize>> = BTreeMap::new();
    let mut context_periods: BTreeMap<_, Vec<usize>> = BTreeMap::new();
    for (i, doc) in documents.iter().enumerate() {
        if !doc.relations_complete {
            flags[i].insert("incompleteRelations");
        }
        if doc.timestamp_ms.is_none() {
            flags[i].insert("undatedSource");
        }
        periods
            .entry((&doc.binding_id, &doc.conversation_id))
            .or_default()
            .push(i);
        context_periods
            .entry(&doc.conversation_id)
            .or_default()
            .push(i);
        if let Some(thread) = &doc.thread_id {
            if let Some(other) = context_threads.insert((&doc.conversation_id, thread), i) {
                leakage.join(i, other);
            }
            if let Some(other) = threads.insert((&doc.binding_id, &doc.conversation_id, thread), i)
            {
                assembly.join(i, other);
                leakage.join(i, other);
            }
        }
        // Equal text is never deleted. Conservatively tie its partition, including
        // tiny repeated replies, while keeping every original reference intact.
        for other in [
            sources.insert(&doc.source_key, i),
            identities.insert(&doc.reference.document_id, i),
            if doc.text.is_empty() {
                None
            } else {
                copies.insert(&doc.text, i)
            },
        ]
        .into_iter()
        .flatten()
        {
            leakage.join(i, other);
        }
        for relation in &doc.related {
            if let Some(&other) =
                refs.get(&(relation.document_id.as_str(), relation.revision_id.as_str()))
            {
                leakage.join(i, other);
                if doc.binding_id == documents[other].binding_id
                    && doc.conversation_id == documents[other].conversation_id
                {
                    assembly.join(i, other);
                }
            } else {
                flags[i].insert("unresolvedRelation");
            }
        }
    }
    for members in periods.values_mut() {
        members.sort_by_key(|&i| {
            (
                documents[i].timestamp_ms,
                &documents[i].reference.document_id,
                &documents[i].reference.revision_id,
            )
        });
        for pair in members.windows(2) {
            if let (Some(a), Some(b)) = (
                documents[pair[0]].timestamp_ms,
                documents[pair[1]].timestamp_ms,
            ) {
                if b.checked_sub(a)
                    .is_some_and(|gap| gap <= policy.time_gap_ms)
                {
                    assembly.join(pair[0], pair[1]);
                    leakage.join(pair[0], pair[1]);
                }
            }
        }
    }
    // Binding boundaries retain authority/attribution, but never permit splitting
    // adjacent speakers or a thread into different evaluation partitions.
    for members in context_periods.values_mut() {
        members.sort_by_key(|&i| documents[i].timestamp_ms);
        for pair in members.windows(2) {
            if let (Some(a), Some(b)) = (
                documents[pair[0]].timestamp_ms,
                documents[pair[1]].timestamp_ms,
            ) {
                if b.checked_sub(a)
                    .is_some_and(|gap| gap <= policy.time_gap_ms)
                {
                    leakage.join(pair[0], pair[1]);
                }
            }
        }
    }
    let mut linked: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut assembled: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..documents.len() {
        linked.entry(leakage.root(i)).or_default().push(i);
        assembled.entry(assembly.root(i)).or_default().push(i);
    }
    let policy_hash = digest(
        serde_json::to_vec(&(
            policy.time_gap_ms,
            policy.holdout_basis_points,
            &policy.seed,
        ))
        .map_err(|_| INVALID)?,
    );
    let split_version = split_fingerprint(documents, &policy_hash)?;
    let mut assignments = HashMap::new();
    for (root, members) in &mut linked {
        members.sort_by_key(|&i| {
            (
                &documents[i].reference.document_id,
                &documents[i].reference.revision_id,
            )
        });
        let references: Vec<_> = members.iter().map(|&i| &documents[i].reference).collect();
        let hash = digest(serde_json::to_vec(&(&policy_hash, references)).map_err(|_| INVALID)?);
        let bucket = u32::from_str_radix(&hash[..8], 16).map_err(|_| INVALID)? % 10000;
        let limitations: BTreeSet<_> = members
            .iter()
            .flat_map(|&i| flags[i].iter().copied())
            .collect();
        let holdout = bucket < u32::from(policy.holdout_basis_points)
            || !limitations.is_empty()
            || members.iter().any(|&i| documents[i].holdout);
        assignments.insert(
            *root,
            (
                if holdout {
                    Partition::Holdout
                } else {
                    Partition::Reference
                },
                limitations,
            ),
        );
    }
    let mut output = Vec::new();
    for (_, mut members) in assembled {
        members.sort_by_key(|&i| {
            (
                documents[i].timestamp_ms,
                &documents[i].reference.document_id,
                &documents[i].reference.revision_id,
            )
        });
        let first = &documents[members[0]];
        let (partition, limitations) = &assignments[&leakage.root(members[0])];
        let document_refs: Vec<_> = members
            .iter()
            .map(|&i| documents[i].reference.clone())
            .collect();
        let id = format!(
            "episode:{}",
            digest(
                serde_json::to_vec(&(
                    snapshot_id,
                    &split_version,
                    &first.binding_id,
                    &first.conversation_id,
                    &document_refs
                ))
                .map_err(|_| INVALID)?
            )
        );
        let thread_id = if members
            .iter()
            .all(|&i| documents[i].thread_id == first.thread_id)
        {
            first.thread_id.clone()
        } else {
            None
        };
        output.push((
            PreparedEpisode {
                schema_version: 1,
                id,
                snapshot_id: snapshot_id.into(),
                binding_id: first.binding_id.clone(),
                conversation_id: first.conversation_id.clone(),
                thread_id,
                document_refs,
                partition: *partition,
                split_version: split_version.clone(),
                assembly_version: "episodes:1".into(),
                limitations: limitations.iter().map(|s| (*s).into()).collect(),
            },
            members,
        ));
    }
    output.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let (episodes, members) = output.into_iter().unzip();
    Ok(PreparedEpisodes {
        documents,
        episodes,
        members,
    })
}
// The receipt pins declared assignments and graph inputs as well as the policy.
// Sampling uses only the policy and connected refs, so adding an unrelated group
// does not reshuffle an existing group's random assignment.
fn split_fingerprint(documents: &[EpisodeDocument], policy_hash: &str) -> Result<String, String> {
    let mut ordered: Vec<_> = documents.iter().collect();
    ordered.sort_by_key(|d| (&d.reference.document_id, &d.reference.revision_id));
    let mut hashes = Vec::with_capacity(ordered.len());
    for doc in ordered {
        let mut related: Vec<_> = doc.related.iter().collect();
        related.sort_by_key(|r| (&r.document_id, &r.revision_id));
        related.dedup();
        hashes.push(digest(
            serde_json::to_vec(&(
                &doc.reference,
                &doc.source_key,
                &doc.binding_id,
                &doc.conversation_id,
                &doc.thread_id,
                doc.timestamp_ms,
                digest(&doc.text),
                related,
                doc.relations_complete,
                doc.holdout,
            ))
            .map_err(|_| INVALID)?,
        ));
    }
    Ok(format!(
        "episodeSplit:1:{}",
        digest(serde_json::to_vec(&(policy_hash, hashes)).map_err(|_| INVALID)?)
    ))
}
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn validate(
    snapshot: &str,
    documents: &[EpisodeDocument],
    policy: &EpisodePolicy,
) -> Result<(), String> {
    if !valid_id(snapshot)
        || !valid_id(&policy.seed)
        || !(0..=31_536_000_000).contains(&policy.time_gap_ms)
        || policy.holdout_basis_points > 10000
        || documents.len() > 100_000
    {
        return Err(INVALID.into());
    }
    let mut refs = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    let mut bytes = 0usize;
    let mut relations = 0usize;
    for doc in documents {
        bytes = bytes.checked_add(doc.text.len()).ok_or(INVALID)?;
        relations = relations.checked_add(doc.related.len()).ok_or(INVALID)?;
        if [
            &doc.reference.document_id,
            &doc.reference.revision_id,
            &doc.source_key,
            &doc.binding_id,
            &doc.conversation_id,
        ]
        .into_iter()
        .any(|s| !valid_id(s))
            || doc.thread_id.as_ref().is_some_and(|s| !valid_id(s))
            || doc
                .timestamp_ms
                .is_some_and(|ms| !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&ms))
            || doc.text.len() > 4 * 1024 * 1024
            || bytes > 128 * 1024 * 1024
            || doc.related.len() > 128
            || relations > 1_000_000
            || !refs.insert((&doc.reference.document_id, &doc.reference.revision_id))
            || !revisions.insert(&doc.reference.revision_id)
            || doc
                .related
                .iter()
                .any(|r| !valid_id(&r.document_id) || !valid_id(&r.revision_id))
        {
            return Err(INVALID.into());
        }
    }
    Ok(())
}
// Iterative union-by-size: long reply chains cannot overflow the call stack.
struct Groups {
    parents: Vec<usize>,
    sizes: Vec<usize>,
}
impl Groups {
    fn new(n: usize) -> Self {
        Self {
            parents: (0..n).collect(),
            sizes: vec![1; n],
        }
    }
    fn root(&mut self, mut i: usize) -> usize {
        while self.parents[i] != i {
            self.parents[i] = self.parents[self.parents[i]];
            i = self.parents[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.root(a), self.root(b));
        if a == b {
            return;
        }
        if self.sizes[a] < self.sizes[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parents[b] = a;
        self.sizes[a] += self.sizes[b];
    }
}
#[cfg(test)]
#[path = "corpus-episode-tests.rs"]
mod tests;
