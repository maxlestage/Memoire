//! Documentation search, shared by both sides. The server writes one index per language
//! (`/search/{lang}.txt`: every page with its plain text, a small file, so search also
//! works on a static host and offline); the browser downloads it on the first key press
//! and searches it locally on every key press.

/// One documentation page of the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub slug: String,
    pub title: String,
    /// The page's category ("Core concepts").
    pub category: String,
    /// The plain text of the page, on one line.
    pub text: String,
    /// `title` and `text` lowercased and without accents ([`fold`]).
    folded_title: String,
    folded_text: String,
}

/// A page matching a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit<'a> {
    pub entry: &'a Entry,
    /// The title contains the query: listed first.
    pub title_match: bool,
    /// `(before, match, after)`: an excerpt around the first match in the text.
    pub snippet: Option<(String, String, String)>,
}

impl Entry {
    pub fn new(slug: &str, title: &str, category: &str, text: &str) -> Entry {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let (title, text) = (clean(title), clean(text));
        Entry {
            slug: clean(slug),
            category: clean(category),
            folded_title: fold(&title),
            folded_text: fold(&text),
            title,
            text,
        }
    }

    /// One line of the index: `slug \t title \t category \t text \n`.
    pub fn line(&self) -> String {
        format!("{}\t{}\t{}\t{}\n", self.slug, self.title, self.category, self.text)
    }

    pub fn parse(line: &str) -> Option<Entry> {
        let mut fields = line.trim_end_matches(['\n', '\r']).splitn(4, '\t');
        let (slug, title, category, text) = (fields.next()?, fields.next()?, fields.next()?, fields.next()?);
        Some(Entry::new(slug, title, category, text))
    }
}

/// The pages containing every word of `query` (case and accents ignored) in their title or
/// text: the pages whose title has them all first, in reading order otherwise. The snippet
/// shows the whole query if the text has it, else its first word found. An empty query
/// finds nothing.
pub fn search<'a>(entries: &'a [Entry], query: &str) -> Vec<Hit<'a>> {
    let q = fold(query.trim());
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let phrase = words.join(" ");
    let mut hits: Vec<Hit<'a>> = entries
        .iter()
        .filter(|e| words.iter().all(|w| e.folded_title.contains(w) || e.folded_text.contains(w)))
        .map(|entry| {
            let title_match = words.iter().all(|w| entry.folded_title.contains(w));
            let found = entry
                .folded_text
                .find(&phrase)
                .map(|pos| (pos, phrase.len()))
                .or_else(|| words.iter().find_map(|w| entry.folded_text.find(w).map(|pos| (pos, w.len()))));
            let snippet = found.map(|(pos, len)| excerpt(&entry.text, pos, len));
            Hit { entry, title_match, snippet }
        })
        .collect();
    // Stable: equal ranks keep the reading order.
    hits.sort_by_key(|h| !h.title_match);
    hits
}

/// Lowercase, accents removed (`"Déployer Œuvre"` → `"deployer oeuvre"`).
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        push_folded(&mut out, c);
    }
    out
}

fn push_folded(out: &mut String, c: char) {
    for lower in c.to_lowercase() {
        match deaccent(lower) {
            Some(ascii) => out.push_str(ascii),
            None => out.push(lower),
        }
    }
}

fn folded_len(c: char) -> usize {
    let mut s = String::new();
    push_folded(&mut s, c);
    s.len()
}

/// The byte offset in `text` of the position `target` of `fold(text)`: the start of the
/// character it falls in (`ceil`: the end of that character).
fn original_offset(text: &str, target: usize, ceil: bool) -> usize {
    let mut acc = 0;
    for (i, c) in text.char_indices() {
        if acc >= target {
            return i;
        }
        let len = folded_len(c);
        if acc + len > target {
            return if ceil { i + c.len_utf8() } else { i };
        }
        acc += len;
    }
    text.len()
}

/// `(before, match, after)` around the match at `pos..pos + len` of the folded text.
fn excerpt(text: &str, pos: usize, len: usize) -> (String, String, String) {
    let start = original_offset(text, pos, false);
    let end = original_offset(text, pos + len, true).max(start);
    let from = text[..start].char_indices().rev().nth(47).map_or(0, |(i, _)| i);
    let to = text[end..].char_indices().nth(72).map_or(text.len(), |(i, _)| end + i);
    (
        format!("{}{}", if from > 0 { "…" } else { "" }, &text[from..start]),
        text[start..end].to_string(),
        format!("{}{}", &text[end..to], if to < text.len() { "…" } else { "" }),
    )
}

/// The ASCII spelling of an accented Latin letter, `None` for any other character.
pub fn deaccent(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
        'ç' | 'ć' | 'č' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' => "u",
        'ý' | 'ÿ' => "y",
        'š' | 'ś' => "s",
        'ž' | 'ź' | 'ż' => "z",
        'œ' => "oe",
        'æ' => "ae",
        'ß' => "ss",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Vec<Entry> {
        vec![
            Entry::new("intro", "Introduction", "Getting started", "Memoire maps structs to tables."),
            Entry::new("heroku", "Déployer sur Heroku", "Déploiement", "Un dyno Heroku et un zèbre."),
            Entry::new("faq", "FAQ", "Référence", "Peut-on déployer ailleurs ? Oui."),
        ]
    }

    #[test]
    fn titles_first_accents_and_case_ignored() {
        let index = index();
        let slugs = |q: &str| search(&index, q).iter().map(|h| h.entry.slug.as_str()).collect::<Vec<_>>();
        assert_eq!(slugs("DEPLOYER"), ["heroku", "faq"]);
        assert_eq!(slugs("zebre"), ["heroku"]);
        assert_eq!(slugs("heroku  deployer"), ["heroku"]);
        assert!(slugs("   ").is_empty());
        assert!(slugs("nothing").is_empty());
    }

    #[test]
    fn snippets_keep_the_original_spelling() {
        let index = index();
        let hits = search(&index, "zebre");
        let (before, matched, after) = hits[0].snippet.clone().unwrap();
        assert_eq!(matched, "zèbre");
        assert_eq!(before, "Un dyno Heroku et un ");
        assert_eq!(after, ".");
        let long = Entry::new("x", "X", "C", &format!("{} œuvre {}", "a ".repeat(60), "b ".repeat(60)));
        let hit = search(std::slice::from_ref(&long), "OEUVRE").remove(0);
        let (before, matched, after) = hit.snippet.unwrap();
        assert_eq!(matched, "œuvre");
        assert!(before.starts_with('…') && after.ends_with('…'), "{before:?} {after:?}");
    }

    #[test]
    fn index_lines_round_trip() {
        let entry = Entry::new("a", "Ti\ttle", "Cat", "Some\ntext");
        let back = Entry::parse(&entry.line()).unwrap();
        assert_eq!(back, entry);
        assert_eq!(back.title, "Ti tle");
        assert!(Entry::parse("only\ttwo").is_none());
    }
}
