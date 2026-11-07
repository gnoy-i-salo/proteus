//! Deterministic query-term parsing for the corpus FTS search.
//!
//! A query is a sequence of terms separated by whitespace or neutral
//! punctuation. A term retains Unicode letters, digits, combining marks and `_`; a single
//! trailing `*` turns it into a prefix term. FTS5 syntax characters are
//! rejected outright, so a parsed query reaches the engine only as validated
//! terms joined by implicit AND. Every term is rendered quoted (`"term"`,
//! `"term" *`): natural words like `and`/`or`/`not`/`near` stay searchable,
//! and no bare token ever goes through the FTS5 query parser's tokenizer,
//! which would treat operator words and glob-looking sequences as syntax.
use super::{MAX_QUERY_BYTES, MAX_TERMS, MAX_TERM_BYTES};
const MALFORMED: &str = "Поисковый запрос содержит недопустимые символы.";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParsedTerm {
    pub(super) text: String,
    pub(super) prefix: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParsedQuery {
    pub(super) terms: Vec<ParsedTerm>,
}
fn is_term_char(character: char) -> bool {
    if character.is_alphanumeric() || character == '_' {
        return true;
    }
    // Keep marks attached to their letters. Splitting NFD words here would
    // disagree with FTS5 tokenization; original/query bytes are never rewritten.
    static MARK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^\p{M}$").expect("fixed Unicode mark category is valid")
    });
    let mut bytes = [0; 4];
    MARK.is_match(character.encode_utf8(&mut bytes))
}
pub(super) fn parse_query(query: &str) -> Result<ParsedQuery, String> {
    if query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err("Поисковый запрос пуст или превышает допустимый размер.".into());
    }
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut starred = false;
    for character in query.chars() {
        if character.is_whitespace() {
            finish_term(&mut terms, &mut current, &mut starred)?;
            continue;
        }
        if is_term_char(character) {
            if starred {
                // A term character directly after `*` would broaden into FTS5
                // glob syntax; only one trailing star per term is defined.
                return Err(MALFORMED.into());
            }
            current.push(character);
            continue;
        }
        if character == '*' {
            if starred || current.is_empty() {
                return Err(MALFORMED.into());
            }
            starred = true;
            continue;
        }
        if character.is_control() {
            return Err(MALFORMED.into());
        }
        if matches!(
            character,
            '"' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '\\'
        ) {
            return Err(MALFORMED.into());
        }
        // Neutral punctuation (including `:` and `/` inside URLs) is a
        // deterministic term separator, never FTS5 syntax: it ANDs terms
        // without widening the user selection.
        finish_term(&mut terms, &mut current, &mut starred)?;
    }
    finish_term(&mut terms, &mut current, &mut starred)?;
    if terms.is_empty() {
        return Err("Поисковый запрос не содержит терминов.".into());
    }
    Ok(ParsedQuery { terms })
}
fn finish_term(
    terms: &mut Vec<ParsedTerm>,
    current: &mut String,
    starred: &mut bool,
) -> Result<(), String> {
    if current.is_empty() {
        *starred = false;
        return Ok(());
    }
    if !current
        .chars()
        .any(|character| character.is_alphanumeric() || character == '_')
    {
        return Err("Поисковый запрос не содержит терминов.".into());
    }
    if terms.len() >= MAX_TERMS {
        return Err("Слишком много терминов в поисковом запросе.".into());
    }
    if current.len() > MAX_TERM_BYTES {
        return Err("Термин поискового запроса превышает допустимый размер.".into());
    }
    // Operator-looking words (and/or/not/near) are ordinary source words;
    // quoting in `fts5_expression` keeps them searchable.
    terms.push(ParsedTerm {
        text: std::mem::take(current),
        prefix: std::mem::take(starred),
    });
    Ok(())
}
/// Render validated terms as an FTS5 MATCH expression. Every term is quoted
/// (`"term"` exact, `"term" *` prefix): strings are never reinterpreted as
/// FTS5 syntax, so natural words and tokenizer edge cases cannot diverge.
pub(super) fn fts5_expression(query: &ParsedQuery) -> String {
    query
        .terms
        .iter()
        .map(|term| {
            if term.prefix {
                format!("\"{}\" *", term.text)
            } else {
                format!("\"{}\"", term.text)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
