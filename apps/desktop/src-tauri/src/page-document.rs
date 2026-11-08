//! Canonical page envelope. Acquisition and permission checks belong to the caller.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
const INVALID: &str = "Ревизия страницы не соответствует исходнику или не поддерживается.";
const MAX_ENCODED_BYTES: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PageOrigin {
    SelectedUrl {
        selected_url: String,
        final_url: String,
        redirects: Vec<String>,
        retrieved_at: String,
    },
    SavedPage {
        source_origin: String,
        label: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageDocument {
    pub schema_version: u8,
    pub id: String,
    pub source_key: String,
    pub origin: PageOrigin,
    pub content_type: String,
    pub original: String,
    pub sha256: String,
    pub extraction_version: String,
    pub text: String,
    pub title: Option<String>,
    pub author_labels: Vec<String>,
    pub published_at: Option<String>,
    pub limitations: Vec<String>,
}

pub fn normalize(
    origin: PageOrigin,
    content_type: String,
    original: String,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageDocument, String> {
    check()?;
    let source_key = source_key(&origin)?;
    let content = super::selected_page_content::extract(&original, &content_type, check)?;
    check()?;
    let sha256 = format!("{:x}", Sha256::digest(original.as_bytes()));
    check()?;
    let document = PageDocument {
        schema_version: 1,
        id: format!("document:{source_key}"),
        source_key,
        origin,
        content_type,
        original,
        sha256,
        extraction_version: super::selected_page_content::EXTRACTOR_VERSION.into(),
        text: content.text,
        title: content.title,
        author_labels: content.author_labels,
        published_at: content.published_at,
        limitations: content.limitations,
    };
    // Count escaped JSON bytes without allocating a second envelope. Every new
    // document must fit the decoder, including worst-case control characters.
    encoded_size(&document)?;
    check()?;
    Ok(document)
}

pub fn encoded_size(document: &PageDocument) -> Result<usize, String> {
    let mut counter = EnvelopeSize(0);
    serde_json::to_writer(&mut counter, document).map_err(|_| INVALID)?;
    Ok(counter.0)
}

pub(super) fn source_key(origin: &PageOrigin) -> Result<String, String> {
    let (provider, identity) = origin_identity(origin)?;
    let identity_bytes = serde_json::to_vec(&(provider, identity)).map_err(|_| INVALID)?;
    Ok(format!(
        "pageSource:1:{provider}:{:x}",
        Sha256::digest(identity_bytes)
    ))
}

struct EnvelopeSize(usize);
impl std::io::Write for EnvelopeSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_ENCODED_BYTES)
            .ok_or_else(|| std::io::Error::other(INVALID))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// This checks stored semantics, not permission or acquisition authenticity.
/// The host must authorize the snapshot and verify its revision hash first.
pub fn decode(
    encoded: &str,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageDocument, String> {
    check()?;
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(INVALID.into());
    }
    let document: PageDocument = serde_json::from_str(encoded).map_err(|_| INVALID)?;
    if document.schema_version != 1
        || document.extraction_version != super::selected_page_content::EXTRACTOR_VERSION
    {
        return Err(INVALID.into());
    }
    check()?;
    let expected = normalize(
        document.origin.clone(),
        document.content_type.clone(),
        document.original.clone(),
        check,
    )?;
    // Keep required null fields; serde's Option alone accepts absent fields.
    // Typed deserialization above rejects duplicate keys before Value comparison.
    if document != expected
        || serde_json::from_str::<serde_json::Value>(encoded).map_err(|_| INVALID)?
            != serde_json::to_value(&expected).map_err(|_| INVALID)?
    {
        return Err(INVALID.into());
    }
    check()?;
    Ok(document)
}

fn origin_identity(origin: &PageOrigin) -> Result<(&'static str, String), String> {
    match origin {
        PageOrigin::SavedPage {
            source_origin,
            label,
        } => {
            super::input::validate_origin(source_origin)?;
            if label.is_empty() || label.len() > 65536 {
                return Err(INVALID.into());
            }
            Ok(("savedPage", source_origin.clone()))
        }
        PageOrigin::SelectedUrl {
            selected_url,
            final_url,
            redirects,
            retrieved_at,
        } => {
            let selected = canonical_url(selected_url)?;
            if redirects.len() > 5
                || final_url != &canonical_url(final_url)?
                || retrieved_at.len() > 128
            {
                return Err(INVALID.into());
            }
            let time = chrono::DateTime::parse_from_rfc3339(retrieved_at).map_err(|_| INVALID)?;
            if !(0..=9_007_199_254_740_991).contains(&time.timestamp_millis()) {
                return Err(INVALID.into());
            }
            let mut prior = selected.as_str();
            for target in redirects {
                if target != &canonical_url(target)?
                    || (prior.starts_with("https:") && !target.starts_with("https:"))
                {
                    return Err(INVALID.into());
                }
                prior = target;
            }
            if prior != final_url {
                return Err(INVALID.into());
            }
            Ok(("web", selected))
        }
    }
}
// A deterministic identity/shape check only. No DNS, network or access decision:
// acquisition enforces the public-address policy before producing this envelope.
fn canonical_url(raw: &str) -> Result<String, String> {
    if raw.len() > 8192 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return Err(INVALID.into());
    }
    let mut url = url::Url::parse(raw).map_err(|_| INVALID)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(INVALID.into());
    }
    url.set_fragment(None);
    Ok(url.into())
}
#[cfg(test)]
#[path = "page-document-tests.rs"]
mod tests;
