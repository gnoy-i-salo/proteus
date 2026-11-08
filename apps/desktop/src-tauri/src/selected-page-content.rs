//! A versioned text projection, separate from the immutable original page bytes.
use dom_query::{Document, NodeRef};
use html5ever::tendril::TendrilSink;
pub const EXTRACTOR_VERSION: &str = "selectedPageText:1:domQuery0.28.0:html5ever0.39.0";
pub(super) const MAX_BYTES: usize = 4 * 1024 * 1024;
const LIMIT: &str = "Страница превышает ограничения разбора текста.";
#[derive(Debug, PartialEq, Eq)]
pub struct PageContent {
    pub text: String,
    pub title: Option<String>,
    pub author_labels: Vec<String>,
    pub published_at: Option<String>,
    pub limitations: Vec<String>,
}

pub fn extract(
    original: &str,
    content_type: &str,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageContent, String> {
    check()?;
    if original.len() > MAX_BYTES || original.trim().is_empty() {
        return Err(LIMIT.into());
    }
    if matches!(content_type, "text/plain" | "text/markdown") {
        let result = PageContent {
            text: original.into(),
            title: None,
            author_labels: vec![],
            published_at: None,
            limitations: vec!["publicationTimeUnavailable".into()],
        };
        check()?;
        return Ok(result);
    }
    if content_type != "text/html" {
        return Err("Неподдерживаемый формат страницы.".into());
    }
    // A conservative source-markup cap also counts '<' in comments/attributes.
    // It bounds parsing work before allocating a DOM, without interpreting HTML
    // through a regex or pretending the marker count is a DOM-node count.
    if original
        .bytes()
        .filter(|byte| *byte == b'<')
        .take(20001)
        .count()
        > 20000
    {
        return Err(LIMIT.into());
    }
    let mut parser = html5ever::parse_document(
        Document::default(),
        html5ever::ParseOpts {
            tree_builder: html5ever::tree_builder::TreeBuilderOpts {
                scripting_enabled: false,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut repaired = false;
    let mut start = 0;
    while start < original.len() {
        check()?;
        let mut end = (start + 4096).min(original.len());
        while !original.is_char_boundary(end) {
            end -= 1;
        }
        parser.process(original[start..end].into());
        let mut errors = parser.tokenizer.sink.sink.errors.borrow_mut();
        repaired |= !errors.is_empty();
        errors.clear();
        check()?;
        start = end;
    }
    let document = parser.finish();
    repaired |= !document.errors.borrow().is_empty();
    check()?;
    if !document
        .select("input[type='password' i]")
        .nodes()
        .is_empty()
    {
        return Err(
            "Страница содержит форму входа. Используйте явно выбранный сохранённый источник."
                .into(),
        );
    }
    let mut result = metadata(&document, check)?;
    if repaired {
        result.limitations.push("htmlMarkupRepaired".into());
    }
    let body = document
        .body()
        .ok_or("В HTML нет текстового содержимого.")?;
    result.text = body_text(body, check)?;
    if result.text.trim().is_empty() {
        return Err("В странице нет доступного текстового содержимого.".into());
    }
    check()?;
    Ok(result)
}

fn metadata(
    document: &Document,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageContent, String> {
    let mut titles = Vec::new();
    for title in document.select("title").nodes() {
        check()?;
        unique_metadata(&mut titles, title.text().to_string())?;
    }
    let mut authors = Vec::new();
    let mut dates = Vec::new();
    for meta in document.select("meta").nodes() {
        check()?;
        let Some(value) = meta.attr("content") else {
            continue;
        };
        if meta
            .attr("name")
            .is_some_and(|name| name.eq_ignore_ascii_case("author"))
        {
            unique_metadata(&mut authors, value.to_string())?;
        }
        if meta
            .attr("property")
            .is_some_and(|name| name.eq_ignore_ascii_case("article:published_time"))
        {
            unique_metadata(&mut dates, value.to_string())?;
        }
    }
    let published_at = (dates.len() == 1)
        .then(|| dates[0].clone())
        .filter(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok());
    let mut limitations = vec!["htmlTextProjection".into(), "cssLayoutNotEvaluated".into()];
    if !authors.is_empty() || !dates.is_empty() {
        limitations.push("publisherMetadataUnverified".into());
    }
    if published_at.is_none() {
        limitations.push("publicationTimeUnavailable".into());
    }
    let title = (titles.len() == 1).then(|| titles.remove(0));
    Ok(PageContent {
        text: String::new(),
        title,
        author_labels: authors,
        published_at,
        limitations,
    })
}

fn unique_metadata(values: &mut Vec<String>, value: String) -> Result<(), String> {
    if value.len() > 2048 {
        return Err(LIMIT.into());
    }
    if !value.trim().is_empty() && !values.contains(&value) {
        if values.len() >= 32 {
            return Err(LIMIT.into());
        }
        values.push(value);
    }
    Ok(())
}

fn body_text(
    root: NodeRef<'_>,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<String, String> {
    let mut pending = vec![(root, false, 0usize)];
    let mut text = String::new();
    let mut visited = 0usize;
    while let Some((node, closing, depth)) = pending.pop() {
        check()?;
        visited += 1;
        if visited > 100000 || depth > 256 {
            return Err(LIMIT.into());
        }
        if node.is_text() {
            append(&mut text, &node.text())?;
            continue;
        }
        let name = node.node_name().unwrap_or_default();
        if closing {
            if block(&name) {
                line(&mut text)?;
            } else if matches!(name.as_ref(), "td" | "th") {
                append(&mut text, "\t")?;
            }
            continue;
        }
        if matches!(
            name.as_ref(),
            "script" | "style" | "template" | "iframe" | "object" | "embed" | "svg" | "math"
        ) || node.attr("hidden").is_some()
            || node
                .attr("aria-hidden")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        {
            continue;
        }
        if name.as_ref() == "br" {
            append(&mut text, "\n")?;
            continue;
        }
        if block(&name) && !text.is_empty() {
            line(&mut text)?;
        }
        pending.push((node, true, depth));
        for child in node.children_it(true) {
            pending.push((child, false, depth + 1));
        }
    }
    Ok(text)
}
fn append(text: &mut String, value: &str) -> Result<(), String> {
    if value.len() > MAX_BYTES - text.len() {
        return Err(LIMIT.into());
    }
    text.push_str(value);
    Ok(())
}
fn line(text: &mut String) -> Result<(), String> {
    if !text.ends_with('\n') {
        append(text, "\n")?;
    }
    Ok(())
}
fn block(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "blockquote"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "details"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "summary"
            | "table"
            | "tr"
            | "ul"
    )
}
#[cfg(test)]
#[path = "selected-page-content-tests.rs"]
mod tests;
