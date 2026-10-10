//! Markdown → HTML for the documentation, on top of pulldown-cmark.
//!
//! Extras compared to the plain renderer:
//! * headings get stable ids + anchor links, and h2/h3 feed the "On this page" TOC;
//! * fenced code blocks are syntax-highlighted and get a copy button;
//! * blockquotes starting with **Note** / **Tip** / **Warning** (or GitHub's
//!   `[!NOTE]` syntax) become callouts;
//! * tables are wrapped for horizontal scrolling on phones;
//! * links to `other-page.md` are rewritten to `/{lang}/docs/other-page/`, external links
//!   open in a new tab.

use std::collections::HashMap;

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::highlight::{escape_html, highlight};
use crate::routes::{Lang, link};

#[derive(Clone, PartialEq, Debug)]
pub struct TocEntry {
    pub level: u8,
    pub id: String,
    pub text: String,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Rendered {
    pub html: String,
    pub toc: Vec<TocEntry>,
    pub words: usize,
}

/// Localised labels injected into the generated HTML.
pub struct Labels<'a> {
    pub copy: &'a str,
    pub note: &'a str,
    pub tip: &'a str,
    pub warning: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Callout {
    Note,
    Tip,
    Warning,
}

impl Callout {
    fn class(self) -> &'static str {
        match self {
            Callout::Note => "note",
            Callout::Tip => "tip",
            Callout::Warning => "warning",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Callout::Note => {
                r#"<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>"#
            }
            Callout::Tip => {
                r#"<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z"/></svg>"#
            }
            Callout::Warning => {
                r#"<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01"/></svg>"#
            }
        }
    }

    fn label<'a>(self, labels: &Labels<'a>) -> &'a str {
        match self {
            Callout::Note => labels.note,
            Callout::Tip => labels.tip,
            Callout::Warning => labels.warning,
        }
    }

    /// Recognises the bold keyword that opens a callout, in any of the site languages.
    fn from_keyword(word: &str) -> Option<Callout> {
        let w = word.trim().trim_end_matches(':').trim().to_lowercase();
        match w.as_str() {
            "note" | "remarque" | "nota" | "info" | "important" => Some(Callout::Note),
            "tip" | "astuce" | "conseil" | "consejo" | "truco" => Some(Callout::Tip),
            "warning" | "attention" | "avertissement" | "advertencia" | "atención" | "atencion" | "caution"
            | "danger" | "cuidado" => Some(Callout::Warning),
            _ => None,
        }
    }

    fn from_gfm(kind: BlockQuoteKind) -> Callout {
        match kind {
            BlockQuoteKind::Note | BlockQuoteKind::Important => Callout::Note,
            BlockQuoteKind::Tip => Callout::Tip,
            BlockQuoteKind::Warning | BlockQuoteKind::Caution => Callout::Warning,
        }
    }
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_GFM
        | Options::ENABLE_FOOTNOTES
}

/// Heading text → URL fragment (`"Créer une table"` → `"creer-une-table"`).
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        let mapped: Option<&str> = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => Some("a"),
            'ç' => Some("c"),
            'é' | 'è' | 'ê' | 'ë' => Some("e"),
            'í' | 'ì' | 'î' | 'ï' => Some("i"),
            'ñ' => Some("n"),
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => Some("o"),
            'ú' | 'ù' | 'û' | 'ü' => Some("u"),
            'ý' | 'ÿ' => Some("y"),
            'œ' => Some("oe"),
            'æ' => Some("ae"),
            _ => None,
        };
        if let Some(m) = mapped {
            out.push_str(m);
            dash = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if (c.is_whitespace() || c == '-' || c == '_') && !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}

/// Rewrites relative documentation links: `models.md#x` → `/en/docs/models/#x`, `models` →
/// `/en/docs/models/` (behind the base path). Returns the link and whether it is external.
fn rewrite_href(url: &str, lang: Lang) -> (String, bool) {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//") {
        return (url.to_string(), true);
    }
    if lower.starts_with("mailto:") || url.starts_with('#') || url.starts_with('/') {
        return (url.to_string(), false);
    }
    let (path, frag) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let path = path.trim_start_matches("./");
    if let Some(stem) = path.strip_suffix(".md") {
        let slug = stem.rsplit('/').next().unwrap_or(stem);
        return (link(&format!("/{}/docs/{slug}/{frag}", lang.code())), false);
    }
    if !path.is_empty() && !path.contains('/') && !path.contains('.') {
        return (link(&format!("/{}/docs/{path}/{frag}", lang.code())), false);
    }
    (url.to_string(), false)
}

fn heading_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn count_words(s: &str) -> usize {
    s.split_whitespace().count()
}

pub fn render(md: &str, labels: &Labels<'_>, lang: Lang) -> Rendered {
    let events: Vec<Event<'_>> = Parser::new_ext(md, options()).collect();
    let mut out: Vec<Event<'_>> = Vec::with_capacity(events.len() + 16);
    let mut toc = Vec::new();
    let mut used_ids: HashMap<String, usize> = HashMap::new();
    let mut words = 0usize;
    // For each open blockquote: did we turn it into a callout?
    let mut quote_stack: Vec<bool> = Vec::new();
    let mut trim_next_text = false;

    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Heading { level, id, .. }) => {
                let level_n = heading_num(*level);
                let mut j = i + 1;
                let mut text = String::new();
                let mut inner: Vec<Event<'_>> = Vec::new();
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                    match &events[j] {
                        Event::Text(t) | Event::Code(t) => text.push_str(t),
                        _ => {}
                    }
                    inner.push(events[j].clone());
                    j += 1;
                }
                words += count_words(&text);
                let base = id.as_ref().map(|s| s.to_string()).unwrap_or_else(|| slugify(&text));
                let count = used_ids.entry(base.clone()).or_insert(0);
                let anchor = if *count == 0 { base.clone() } else { format!("{base}-{count}") };
                *count += 1;
                let mut inner_html = String::new();
                pulldown_cmark::html::push_html(&mut inner_html, inner.into_iter());
                if level_n == 2 || level_n == 3 {
                    toc.push(TocEntry { level: level_n, id: anchor.clone(), text: text.trim().to_string() });
                }
                let html = if level_n == 1 {
                    format!("<h1 id=\"{anchor}\">{inner_html}</h1>\n")
                } else {
                    format!(
                        "<h{level_n} id=\"{anchor}\">{inner_html}<a class=\"heading-anchor\" href=\"#{anchor}\" aria-label=\"#\">#</a></h{level_n}>\n"
                    )
                };
                out.push(Event::Html(CowStr::from(html)));
                i = j + 1;
                continue;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split([' ', ',', '{']).next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let mut j = i + 1;
                let mut code = String::new();
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::CodeBlock)) {
                    if let Event::Text(t) = &events[j] {
                        code.push_str(t);
                    }
                    j += 1;
                }
                let trimmed = code.trim_end_matches('\n');
                let label = if lang.is_empty() { "text".to_string() } else { lang.clone() };
                let html = format!(
                    "<div class=\"code-block\"><div class=\"code-head\"><span class=\"code-lang\">{}</span><button type=\"button\" class=\"code-copy\">{}</button></div><pre><code class=\"language-{}\">{}</code></pre></div>\n",
                    escape_html(&label),
                    escape_html(labels.copy),
                    escape_html(&label),
                    highlight(trimmed, &lang)
                );
                out.push(Event::Html(CowStr::from(html)));
                i = j + 1;
                continue;
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let callout = match kind {
                    Some(k) => Some(Callout::from_gfm(*k)),
                    None => detect_bold_callout(&events, i),
                };
                match callout {
                    Some(c) => {
                        out.push(Event::Html(CowStr::from(format!(
                            "<div class=\"callout callout-{}\" role=\"note\"><p class=\"callout-title\">{}<span>{}</span></p>\n",
                            c.class(),
                            c.icon(),
                            escape_html(c.label(labels))
                        ))));
                        quote_stack.push(true);
                        if kind.is_none() {
                            // Keep the paragraph, drop **Keyword**, and trim what follows it.
                            out.push(Event::Start(Tag::Paragraph));
                            i += 5;
                            trim_next_text = true;
                            continue;
                        }
                    }
                    None => {
                        quote_stack.push(false);
                        out.push(events[i].clone());
                    }
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if quote_stack.pop().unwrap_or(false) {
                    out.push(Event::Html(CowStr::Borrowed("</div>\n")));
                } else {
                    out.push(events[i].clone());
                }
            }
            Event::SoftBreak | Event::HardBreak if trim_next_text => {
                // "**Note**" alone on its line: skip the line break that follows.
            }
            Event::Text(t) => {
                words += count_words(t);
                if trim_next_text {
                    trim_next_text = false;
                    let rest = t.trim_start_matches([':', ' ', '\u{a0}', '—', '-']);
                    if !rest.is_empty() {
                        out.push(Event::Text(CowStr::from(rest.to_string())));
                    }
                } else {
                    out.push(events[i].clone());
                }
            }
            Event::Start(Tag::Table(_)) => {
                out.push(Event::Html(CowStr::Borrowed("<div class=\"table-wrap\">")));
                out.push(events[i].clone());
            }
            Event::End(TagEnd::Table) => {
                out.push(events[i].clone());
                out.push(Event::Html(CowStr::Borrowed("</div>\n")));
            }
            Event::Start(Tag::Link { dest_url, title, .. }) => {
                let (href, external) = rewrite_href(dest_url, lang);
                let mut a = format!("<a href=\"{}\"", escape_html(&href));
                if !title.is_empty() {
                    a.push_str(&format!(" title=\"{}\"", escape_html(title)));
                }
                if external {
                    a.push_str(" target=\"_blank\" rel=\"noopener noreferrer\"");
                }
                a.push('>');
                out.push(Event::InlineHtml(CowStr::from(a)));
            }
            Event::End(TagEnd::Link) => {
                out.push(Event::InlineHtml(CowStr::Borrowed("</a>")));
            }
            other => {
                if trim_next_text && !matches!(other, Event::SoftBreak) {
                    trim_next_text = false;
                }
                out.push(other.clone());
            }
        }
        i += 1;
    }

    let mut html = String::with_capacity(md.len() * 2);
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    Rendered { html, toc, words }
}

/// `> **Note** text` → Start(BlockQuote) Start(Paragraph) Start(Strong) Text(Note) End(Strong) …
fn detect_bold_callout(events: &[Event<'_>], i: usize) -> Option<Callout> {
    match (events.get(i + 1), events.get(i + 2), events.get(i + 3), events.get(i + 4)) {
        (
            Some(Event::Start(Tag::Paragraph)),
            Some(Event::Start(Tag::Strong)),
            Some(Event::Text(word)),
            Some(Event::End(TagEnd::Strong)),
        ) => Callout::from_keyword(word),
        _ => None,
    }
}

/// The plain text of the first paragraph, at most about 160 characters: the page's
/// `<meta name="description">`.
pub fn summary(md: &str) -> String {
    let mut inside = false;
    let mut out = String::new();
    for ev in Parser::new_ext(md, options()) {
        match ev {
            Event::Start(Tag::Paragraph) => inside = true,
            Event::End(TagEnd::Paragraph) if inside => break,
            Event::Text(t) | Event::Code(t) if inside => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak if inside => out.push(' '),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() <= 160 {
        return out;
    }
    let mut cut: String = out.chars().take(157).collect();
    if let Some(space) = cut.rfind(' ') {
        cut.truncate(space);
    }
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.']))
}

/// Plain text of a markdown document (used by the docs search).
pub fn plain_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for ev in Parser::new_ext(md, options()) {
        match ev {
            Event::Text(t) | Event::Code(t) => {
                out.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak | Event::End(_) => out.push(' '),
            _ => {}
        }
    }
    // collapse whitespace
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: Labels<'static> = Labels { copy: "Copy", note: "Note", tip: "Tip", warning: "Warning" };

    #[test]
    fn headings_and_toc() {
        let r = render("# Title\n\n## Créer une table\n\n### Sub {#custom}\n", &LABELS, Lang::En);
        assert_eq!(r.toc.len(), 2);
        assert_eq!(r.toc[0].id, "creer-une-table");
        assert_eq!(r.toc[1].id, "custom");
        assert!(r.html.contains("<h2 id=\"creer-une-table\">"));
    }

    #[test]
    fn callouts() {
        let r = render("> **Tip** Use MemoryDb in tests.\n", &LABELS, Lang::En);
        assert!(r.html.contains("callout-tip"), "{}", r.html);
        assert!(r.html.contains("Use MemoryDb in tests."));
        assert!(!r.html.contains("<strong>Tip</strong>"));
        let r = render("> [!WARNING]\n> Careful.\n", &LABELS, Lang::En);
        assert!(r.html.contains("callout-warning"), "{}", r.html);
        let r = render("> just a quote\n", &LABELS, Lang::En);
        assert!(r.html.contains("<blockquote>"));
    }

    #[test]
    fn code_and_links() {
        let r = render("```rust\nlet x = 1;\n```\n\n[m](models.md#a) [e](https://x.dev)\n", &LABELS, Lang::Fr);
        assert!(r.html.contains("code-copy"));
        assert!(r.html.contains("href=\"/fr/docs/models/#a\""), "{}", r.html);
        assert!(r.html.contains("target=\"_blank\""));
    }

    #[test]
    fn summaries() {
        assert_eq!(summary("# T\n\nFirst `code` line\nnext.\n\nSecond."), "First code line next.");
        let long = summary(&format!("# T\n\n{}", "word ".repeat(80)));
        assert!(long.ends_with('…') && long.chars().count() <= 160, "{long}");
    }

    #[test]
    fn tables_wrapped() {
        let r = render("| a | b |\n|---|---|\n| 1 | 2 |\n", &LABELS, Lang::En);
        assert!(r.html.contains("<div class=\"table-wrap\"><table>"));
    }
}
