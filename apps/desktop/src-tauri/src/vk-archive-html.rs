//! Bounded parser for VK HTML message archive pages (foundation).
//!
//! Supported shapes and limits are documented in
//! `src/fixtures/vk-archive/README.md`. This module is a reading projection
//! only: it never follows links, never reads attachment files and never
//! touches the network or filesystem. Conversation identity is supplied by
//! the caller from the selected archive relative path, never inferred here.
//! The exact original HTML is retained byte for byte; decoded fields are an
//! explicit presentation projection, not the original.
//!
//! Parsing is driven by the pinned html5ever tokenizer (no DOM expansion):
//! one page is at most 4 MiB of UTF-8, 10000 messages, 100000 tokenizer
//! events and 256 open elements. Unknown or malformed structure is an
//! explicit error; the parser never falls back to presenting whole-page text
//! as parsed messages.

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, ParseError, Tag, TagKind, TagToken, Token, TokenSink, TokenSinkResult, Tokenizer,
    TokenizerOpts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

/// Version of the derived message projection; bumped when projection
/// semantics change. The exact original HTML stays authoritative.
pub const PROJECTION_VERSION: &str = "vkArchiveHtmlMessages:1:html5ever0.39.0";

/// Hard input budget checked before any tokenization (UTF-8 bytes).
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum messages parsed from one page.
pub const MAX_MESSAGES: usize = 10_000;
/// Maximum tokenizer events processed for one page.
pub const MAX_TOKENIZER_EVENTS: usize = 100_000;
/// Maximum open-element depth while tokenizing.
pub const MAX_DEPTH: usize = 256;
/// Derived body projection budget per message.
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// Forwarded/quoted contexts preserved per message.
pub const MAX_FORWARDED_CONTEXTS: usize = 32;
/// Derived body projection budget per forwarded context.
pub const MAX_FORWARDED_BODY_BYTES: usize = 64 * 1024;
/// Attachment metadata blocks per message.
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 32;
/// Decoded attachment description budget.
pub const MAX_ATTACHMENT_TEXT_BYTES: usize = 8 * 1024;
/// Decoded attachment link budget.
pub const MAX_ATTACHMENT_LINK_BYTES: usize = 8 * 1024;
/// Kludge metadata notes per message.
pub const MAX_KLUDGES_PER_MESSAGE: usize = 16;
/// Decoded kludge note budget.
pub const MAX_KLUDGE_TEXT_BYTES: usize = 8 * 1024;
/// Decoded header presentation budget per message.
pub const MAX_HEADER_TEXT_BYTES: usize = 16 * 1024;
/// Decoded author label budget.
pub const MAX_AUTHOR_LABEL_BYTES: usize = 4 * 1024;
/// Decoded author link budget.
pub const MAX_AUTHOR_URL_BYTES: usize = 8 * 1024;
/// Exact external `data-id` budget (kept as a verbatim string).
pub const MAX_EXTERNAL_ID_BYTES: usize = 128;
/// Raw decoded edit label budget.
pub const MAX_EDIT_LABEL_BYTES: usize = 256;
/// How often the incremental checkpoint runs inside the token stream.
const CHECKPOINT_EVENT_INTERVAL: usize = 2048;

const ERR_SIZE: &str = "Превышен лимит размера архива VK HTML.";
const ERR_MESSAGES: &str = "Превышен лимит сообщений архива VK HTML.";
const ERR_EVENTS: &str = "Превышен лимит событий разбора архива VK HTML.";
const ERR_DEPTH: &str = "Превышен лимит вложенности разметки архива VK HTML.";
const ERR_BODY: &str = "Превышен лимит текста сообщения архива VK HTML.";
const ERR_STRUCTURE: &str = "Не удалось распознать архив VK HTML.";

/// One attachment's bounded metadata. Files are never opened or read.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VkAttachment {
    /// Decoded `attachment__link` href, when the export supplies one.
    pub link: Option<String>,
    /// Decoded `attachment__description` text, when present.
    pub description: Option<String>,
}

/// A forwarded or quoted block preserved as its own context. It is never
/// attributed to the enclosing message author.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VkForwardedContext {
    /// Decoded nested author label, `None` when the author is not exposed.
    pub author_label: Option<String>,
    /// Decoded nested author link, when present.
    pub author_url: Option<String>,
    /// Derived text of the forwarded block.
    pub body_text: String,
}

/// Explicit relation markers. Unresolved or partial relations stay marked so
/// episode preparation holds the message out. False means no supported forward
/// marker was detected, not proof of complete reply/quote coverage.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VkMessageRelations {
    /// The message contains a forwarded/quoted nested block.
    pub has_forward_or_quote: bool,
    /// True when relation targets could not be resolved.
    pub unresolved: bool,
    /// Stable machine-readable reasons (camelCase tokens).
    pub reasons: Vec<String>,
}

/// One parsed message. `ordinal` distinguishes identical messages; the
/// parser never deduplicates by content. `external_id` is the exact
/// `data-id` attribute string (never a float conversion); legacy exports
/// without ids stay `None` and are keyed page+ordinal by the caller.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VkArchiveMessage {
    pub ordinal: u32,
    pub external_id: Option<String>,
    /// Decoded author link presentation; not the authenticated identity.
    pub author_url: Option<String>,
    /// Decoded author label presentation.
    pub author_label: Option<String>,
    /// Raw decoded header presentation (author + date + edit label).
    pub header_text: String,
    /// Decoded date/time after removing header comma/edge whitespace; timezone unknown.
    pub date_text: String,
    /// Derived body projection; explicitly not the exact original.
    pub body_text: String,
    pub edited: bool,
    /// Raw decoded edit label, when an edit marker is present.
    pub edit_label: Option<String>,
    pub attachments: Vec<VkAttachment>,
    /// Bounded decoded kludge metadata notes.
    pub kludges: Vec<String>,
    /// Forwarded/quoted blocks as separate context.
    pub forwarded_contexts: Vec<VkForwardedContext>,
    pub relations: VkMessageRelations,
}

/// One parsed page. `original_html` keeps every original byte (CRLF, Unicode,
/// entities, whitespace, source words); everything else is a versioned
/// projection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VkArchivePage {
    pub schema_version: u8,
    pub projection_version: String,
    pub original_html: String,
    pub sha256: String,
    pub messages: Vec<VkArchiveMessage>,
    pub limitations: Vec<String>,
}

/// Parses one VK HTML archive page. `checkpoint` is invoked before work,
/// incrementally during tokenization and at the end; its error aborts.
pub fn parse(
    original: &str,
    checkpoint: &impl Fn() -> Result<(), String>,
) -> Result<VkArchivePage, String> {
    checkpoint()?;
    if original.len() > MAX_INPUT_BYTES {
        return Err(ERR_SIZE.into());
    }
    if original.trim().is_empty() {
        return Err(format!("{ERR_STRUCTURE} Пустая страница."));
    }
    let sink = Sink::new(checkpoint);
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    // Feed the whole page at once so the tokenizer's event stream is a pure
    // function of the input, not of the feed chunking. The tokenizer still
    // works incrementally; the sink checkpoints every few thousand events.
    input.push_back(StrTendril::from(original));
    if let Some(error) = tokenizer.sink.take_error() {
        return Err(error);
    }
    checkpoint()?;
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let sink = tokenizer.sink;
    if let Some(error) = sink.take_error() {
        return Err(error);
    }
    checkpoint()?;
    if sink.has_open_structure() {
        return Err(format!(
            "{ERR_STRUCTURE} Неполная страница: сообщение не закрыто."
        ));
    }
    let messages = sink.take_messages()?;
    let mut limitations = vec![
        "htmlTextProjection".to_string(),
        "dateTimeRawNoTimezone".to_string(),
        "conversationIdentityExternal".to_string(),
        "replyTargetsNotExposed".to_string(),
    ];
    if messages.iter().any(|message| message.relations.unresolved) {
        limitations.push("forwardedRelationsUnresolved".to_string());
    }
    checkpoint()?;
    Ok(VkArchivePage {
        schema_version: 1,
        projection_version: PROJECTION_VERSION.into(),
        original_html: original.to_string(),
        sha256: format!("{:x}", Sha256::digest(original.as_bytes())),
        messages,
        limitations,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    /// Page-level element outside any message; contents ignored.
    Generic,
    /// Raw-text element (script/style/...); contents never become messages.
    Skip,
    Message,
    Header,
    AuthorLink,
    Edited,
    Body,
    Attachment,
    AttachmentLink,
    AttachmentDescription,
    Kludge,
    Forward,
    ForwardHeader,
    ForwardAuthorLink,
    ForwardBody,
}

impl FrameKind {
    fn is_structure(self) -> bool {
        !matches!(self, FrameKind::Generic | FrameKind::Skip)
    }
}

struct Frame {
    name: String,
    kind: FrameKind,
}

struct AttachmentBuilder {
    link: Option<String>,
    description: Option<String>,
}

struct ForwardBuilder {
    author_label: String,
    author_url: Option<String>,
    body_text: String,
    header_seen: bool,
    body_seen: bool,
    author_seen: bool,
}

impl ForwardBuilder {
    fn new() -> Self {
        Self {
            author_label: String::new(),
            author_url: None,
            body_text: String::new(),
            header_seen: false,
            body_seen: false,
            author_seen: false,
        }
    }

    fn finish(self) -> VkForwardedContext {
        let author_label = self.author_label.trim();
        VkForwardedContext {
            author_label: (!author_label.is_empty()).then(|| author_label.to_string()),
            author_url: self.author_url,
            body_text: self.body_text,
        }
    }
}

struct MessageBuilder {
    ordinal: u32,
    external_id: Option<String>,
    author_url: Option<String>,
    author_label: String,
    date_raw: String,
    header_text: String,
    edit_text: String,
    body_text: String,
    attachments: Vec<AttachmentBuilder>,
    kludges: Vec<String>,
    forwards: Vec<ForwardBuilder>,
    edited: bool,
    has_forward: bool,
    header_seen: bool,
    body_seen: bool,
    author_seen: bool,
    edit_seen: bool,
    after_author: bool,
}

impl MessageBuilder {
    fn new(ordinal: u32, external_id: Option<String>) -> Self {
        Self {
            ordinal,
            external_id,
            author_url: None,
            author_label: String::new(),
            date_raw: String::new(),
            header_text: String::new(),
            edit_text: String::new(),
            body_text: String::new(),
            attachments: Vec::new(),
            kludges: Vec::new(),
            forwards: Vec::new(),
            edited: false,
            has_forward: false,
            header_seen: false,
            body_seen: false,
            author_seen: false,
            edit_seen: false,
            after_author: false,
        }
    }

    fn finish(self) -> Result<VkArchiveMessage, String> {
        let author_label = self.author_label.trim();
        if author_label.is_empty() {
            return Err(format!("{ERR_STRUCTURE} Пустое имя автора."));
        }
        let date_text = trim_date(&self.date_raw);
        if date_text.is_empty() {
            return Err(format!("{ERR_STRUCTURE} В заголовке сообщения нет даты."));
        }
        let edit_label = self.edit_text.trim();

        let relations = if self.has_forward {
            VkMessageRelations {
                has_forward_or_quote: true,
                unresolved: true,
                reasons: vec!["forwardedContextPreservedUnresolved".into()],
            }
        } else {
            VkMessageRelations {
                has_forward_or_quote: false,
                unresolved: false,
                reasons: vec![],
            }
        };
        Ok(VkArchiveMessage {
            ordinal: self.ordinal,
            external_id: self.external_id,
            author_url: self.author_url,
            author_label: Some(author_label.to_string()),
            header_text: self.header_text,
            date_text: date_text.to_string(),
            body_text: self.body_text,
            edited: self.edited,
            edit_label: (!edit_label.is_empty()).then(|| edit_label.to_string()),
            attachments: self
                .attachments
                .into_iter()
                .map(|attachment| VkAttachment {
                    link: attachment.link,
                    description: attachment.description.map(|text| text.trim().to_string()),
                })
                .collect(),
            kludges: self
                .kludges
                .into_iter()
                .map(|note| note.trim().to_string())
                .collect(),
            forwarded_contexts: self
                .forwards
                .into_iter()
                .map(ForwardBuilder::finish)
                .collect(),
            relations,
        })
    }
}

fn trim_date(raw: &str) -> &str {
    raw.trim_matches(|c| matches!(c, ',' | ' ' | '\t' | '\n' | '\r'))
}

/// What a run of character tokens feeds, derived from the open frames.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextTarget {
    Discard,
    Body {
        forward: bool,
    },
    HeaderText,
    Author {
        forward: bool,
    },
    Edited,
    AttachmentDescription,
    Kludge,
    /// Text directly inside a message/attachment block: whitespace only.
    BlockLevel,
}

enum Action {
    Continue,
    Raw(RawKind),
}

struct Sink<'a> {
    checkpoint: &'a dyn Fn() -> Result<(), String>,
    events: Cell<usize>,
    error: RefCell<Option<String>>,
    stack: RefCell<Vec<Frame>>,
    messages: RefCell<Vec<MessageBuilder>>,
    seen_ids: RefCell<HashSet<String>>,
}

impl<'a> Sink<'a> {
    fn new(checkpoint: &'a dyn Fn() -> Result<(), String>) -> Self {
        Self {
            checkpoint,
            events: Cell::new(0),
            error: RefCell::new(None),
            stack: RefCell::new(Vec::new()),
            messages: RefCell::new(Vec::new()),
            seen_ids: RefCell::new(HashSet::new()),
        }
    }

    fn failed(&self) -> bool {
        self.error.borrow().is_some()
    }

    fn fail(&self, message: impl Into<String>) {
        let mut error = self.error.borrow_mut();
        if error.is_none() {
            *error = Some(message.into());
        }
    }

    fn take_error(&self) -> Option<String> {
        self.error.borrow_mut().take()
    }

    fn has_open_structure(&self) -> bool {
        self.stack
            .borrow()
            .iter()
            .any(|frame| frame.kind.is_structure())
    }

    fn take_messages(&self) -> Result<Vec<VkArchiveMessage>, String> {
        let builders = self.messages.take();
        if builders.is_empty() {
            return Err(format!(
                "{ERR_STRUCTURE} На странице нет сообщений: разметка не похожа на поддерживаемый архив VK."
            ));
        }
        builders.into_iter().map(MessageBuilder::finish).collect()
    }

    fn push_frame(&self, name: &str, kind: FrameKind) -> bool {
        let mut stack = self.stack.borrow_mut();
        if stack.len() + 1 > MAX_DEPTH {
            self.fail(ERR_DEPTH);
            return false;
        }
        stack.push(Frame {
            name: name.to_string(),
            kind,
        });
        true
    }

    fn append(&self, target: &mut String, text: &str, cap: usize, error: &'static str) {
        if target.len() + text.len() > cap {
            self.fail(error);
            return;
        }
        target.push_str(text);
    }

    fn current_message<R>(&self, f: impl FnOnce(&mut MessageBuilder) -> R) -> Option<R> {
        let mut messages = self.messages.borrow_mut();
        messages.last_mut().map(f)
    }

    fn current_forward<R>(&self, f: impl FnOnce(&mut ForwardBuilder) -> R) -> Option<R> {
        self.current_message(|message| message.forwards.last_mut().map(f))
            .flatten()
    }
}

#[path = "vk-archive-html-grammar.rs"]
mod grammar;
#[path = "vk-archive-html-stream.rs"]
mod stream;

fn tag_name(tag: &Tag) -> &str {
    &tag.name
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    tag.attrs
        .iter()
        .find(|attribute| &*attribute.name.local == name)
        .map(|attribute| &*attribute.value)
}

fn has_class(tag: &Tag, token: &str) -> bool {
    attr(tag, "class")
        .map(|value| value.split_ascii_whitespace().any(|part| part == token))
        .unwrap_or(false)
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Raw-text elements whose contents never become message text.
fn raw_kind(name: &str) -> Option<RawKind> {
    match name {
        "script" => Some(RawKind::ScriptData),
        "style" | "iframe" | "noembed" | "noframes" | "noscript" | "xmp" => Some(RawKind::Rawtext),
        "title" | "textarea" => Some(RawKind::Rcdata),
        _ => None,
    }
}

#[cfg(test)]
#[path = "vk-archive-html-tests.rs"]
mod tests;
