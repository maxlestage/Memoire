//! A tiny hand-written syntax highlighter (Rust, TOML, bash, SQL, JSON).
//!
//! Produces HTML with `<span class="tk-…">` tokens. It is deliberately simple:
//! good-looking for documentation snippets, not a full parser.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    Toml,
    Bash,
    Sql,
    Json,
    Plain,
}

fn lang_of(name: &str) -> Lang {
    match name.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Lang::Rust,
        "toml" => Lang::Toml,
        "bash" | "sh" | "shell" | "console" | "zsh" | "terminal" | "dockerfile" | "yaml"
        | "yml" => Lang::Bash,
        "sql" | "postgres" | "postgresql" | "mysql" | "sqlite" => Lang::Sql,
        "json" | "jsonc" => Lang::Json,
        _ => Lang::Plain,
    }
}

const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while",
];

const RUST_PRIMS: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64", "bool", "char", "str",
];

const BASH_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
    "function", "return", "export", "local", "cd", "echo", "sudo", "cargo", "git", "heroku",
    "rustup", "trunk", "docker", "curl", "npm", "FROM", "RUN", "COPY", "WORKDIR", "CMD", "ENV",
    "ARG", "EXPOSE", "USER", "AS",
];

const SQL_KW: &[&str] = &[
    "select", "from", "where", "and", "or", "not", "insert", "into", "values", "update", "set",
    "delete", "create", "table", "drop", "alter", "add", "column", "primary", "key", "unique",
    "index", "on", "join", "left", "right", "inner", "outer", "order", "by", "asc", "desc",
    "limit", "offset", "group", "having", "as", "null", "is", "like", "escape", "in", "between",
    "begin", "commit", "rollback", "transaction", "returning", "default", "references",
    "foreign", "if", "exists", "integer", "bigint", "text", "varchar", "boolean", "real",
    "serial", "bigserial", "autoincrement", "auto_increment", "count", "distinct", "case",
    "when", "then", "else", "end", "true", "false",
];

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    escape_into(&mut out, s);
    out
}

fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

fn push_tok(out: &mut String, class: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    out.push_str("<span class=\"tk-");
    out.push_str(class);
    out.push_str("\">");
    escape_into(out, text);
    out.push_str("</span>");
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Highlight `code` written in language `lang` (fence info string) into HTML.
pub fn highlight(code: &str, lang: &str) -> String {
    let lang = lang_of(lang);
    if lang == Lang::Plain {
        return escape_html(code);
    }
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(code.len() * 2);
    let mut i = 0;
    let mut line_start = true;
    let slice = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };

    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        // Newlines / whitespace
        if c == '\n' {
            out.push('\n');
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            out.push(c);
            i += 1;
            continue;
        }
        let at_line_start = line_start;
        line_start = false;

        // Comments
        let line_comment = match lang {
            Lang::Rust | Lang::Json => c == '/' && next == Some('/'),
            Lang::Sql => c == '-' && next == Some('-'),
            Lang::Toml | Lang::Bash => c == '#' && (i == 0 || chars[i - 1].is_whitespace()),
            Lang::Plain => false,
        };
        if line_comment {
            let start = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            push_tok(&mut out, "com", &slice(start, i));
            continue;
        }
        if (lang == Lang::Rust || lang == Lang::Sql) && c == '/' && next == Some('*') {
            let start = i;
            i += 2;
            while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(n);
            push_tok(&mut out, "com", &slice(start, i));
            continue;
        }

        // Rust attributes: #[...] / #![...]
        if lang == Lang::Rust && c == '#' && (next == Some('[') || next == Some('!')) {
            let start = i;
            let mut depth = 0i32;
            while i < n {
                match chars[i] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    '\n' => break,
                    _ => {}
                }
                i += 1;
            }
            push_tok(&mut out, "attr", &slice(start, i));
            continue;
        }

        // TOML section headers
        if lang == Lang::Toml && c == '[' && at_line_start {
            let start = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            push_tok(&mut out, "attr", &slice(start, i));
            continue;
        }

        // Rust raw strings r"..." / r#"..."#
        if lang == Lang::Rust && c == 'r' && (next == Some('"') || next == Some('#')) {
            let start = i;
            let mut j = i + 1;
            let mut hashes = 0;
            while j < n && chars[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && chars[j] == '"' {
                j += 1;
                'outer: while j < n {
                    if chars[j] == '"' {
                        let mut k = 0;
                        while k < hashes && j + 1 + k < n && chars[j + 1 + k] == '#' {
                            k += 1;
                        }
                        if k == hashes {
                            j += 1 + hashes;
                            break 'outer;
                        }
                    }
                    j += 1;
                }
                i = j.min(n);
                push_tok(&mut out, "str", &slice(start, i));
                continue;
            }
        }

        // Strings
        let is_quote = match lang {
            Lang::Rust | Lang::Json => c == '"',
            Lang::Toml | Lang::Bash => c == '"' || c == '\'',
            Lang::Sql => c == '\'',
            Lang::Plain => false,
        };
        if is_quote {
            let start = i;
            let quote = c;
            // TOML / Python-like triple quotes
            let triple = lang == Lang::Toml
                && chars.get(i + 1) == Some(&quote)
                && chars.get(i + 2) == Some(&quote);
            if triple {
                i += 3;
                while i < n
                    && !(chars[i] == quote
                        && chars.get(i + 1) == Some(&quote)
                        && chars.get(i + 2) == Some(&quote))
                {
                    i += 1;
                }
                i = (i + 3).min(n);
            } else {
                i += 1;
                while i < n && chars[i] != quote {
                    if chars[i] == '\\' && (quote == '"' || lang == Lang::Rust) {
                        i += 1;
                    }
                    if lang != Lang::Rust && chars.get(i) == Some(&'\n') {
                        break;
                    }
                    i += 1;
                }
                i = (i + 1).min(n);
            }
            push_tok(&mut out, "str", &slice(start, i));
            continue;
        }

        // SQL quoted identifiers "users" / `users`
        if lang == Lang::Sql && (c == '"' || c == '`') {
            let start = i;
            i += 1;
            while i < n && chars[i] != c && chars[i] != '\n' {
                i += 1;
            }
            i = (i + 1).min(n);
            push_tok(&mut out, "ty", &slice(start, i));
            continue;
        }

        // Rust char literals vs lifetimes
        if lang == Lang::Rust && c == '\'' {
            let start = i;
            if next == Some('\\') {
                // escaped char literal '\n'
                let mut j = i + 2;
                while j < n && chars[j] != '\'' && j - i < 12 {
                    j += 1;
                }
                i = (j + 1).min(n);
                push_tok(&mut out, "str", &slice(start, i));
                continue;
            }
            if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                push_tok(&mut out, "str", &slice(start, i));
                continue;
            }
            // lifetime
            i += 1;
            while i < n && is_ident_char(chars[i]) {
                i += 1;
            }
            push_tok(&mut out, "lt", &slice(start, i));
            continue;
        }

        // Placeholders / variables: $1, ?1, $HOME, ${VAR}
        if (lang == Lang::Sql || lang == Lang::Bash) && c == '$' {
            let start = i;
            i += 1;
            if i < n && chars[i] == '{' {
                while i < n && chars[i] != '}' {
                    i += 1;
                }
                i = (i + 1).min(n);
            } else {
                while i < n && is_ident_char(chars[i]) {
                    i += 1;
                }
            }
            push_tok(&mut out, "var", &slice(start, i));
            continue;
        }
        if lang == Lang::Sql && c == '?' {
            let start = i;
            i += 1;
            while i < n && chars[i].is_ascii_digit() {
                i += 1;
            }
            push_tok(&mut out, "var", &slice(start, i));
            continue;
        }

        // Bash flags: --release, -y
        if lang == Lang::Bash
            && c == '-'
            && (i == 0 || chars[i - 1].is_whitespace())
            && next.is_some_and(|x| x == '-' || x.is_alphabetic())
        {
            let start = i;
            while i < n && !chars[i].is_whitespace() && chars[i] != '=' {
                i += 1;
            }
            push_tok(&mut out, "attr", &slice(start, i));
            continue;
        }

        // Numbers
        if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                // stop at range operator `..`
                if chars[i] == '.' && chars.get(i + 1) == Some(&'.') {
                    break;
                }
                if chars[i] == '.' && !chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    break;
                }
                i += 1;
            }
            push_tok(&mut out, "num", &slice(start, i));
            continue;
        }

        // Identifiers / keywords
        if is_ident_start(c) {
            let start = i;
            while i < n && (is_ident_char(chars[i]) || (lang == Lang::Bash && chars[i] == '-')) {
                i += 1;
            }
            let word = slice(start, i);
            let next_c = chars.get(i).copied();
            let class = match lang {
                Lang::Rust => {
                    if next_c == Some('!') && chars.get(i + 1) != Some(&'=') {
                        i += 1;
                        push_tok(&mut out, "mac", &format!("{word}!"));
                        continue;
                    } else if RUST_KW.contains(&word.as_str()) {
                        Some("kw")
                    } else if RUST_PRIMS.contains(&word.as_str()) {
                        Some("ty")
                    } else if word.chars().all(|ch| ch.is_uppercase() || ch == '_' || ch.is_ascii_digit())
                        && word.len() > 1
                    {
                        Some("const")
                    } else if word.starts_with(char::is_uppercase) {
                        Some("ty")
                    } else if next_c == Some('(') {
                        Some("fn")
                    } else {
                        None
                    }
                }
                Lang::Toml => {
                    // key = value
                    let mut j = i;
                    while j < n && (chars[j] == ' ' || chars[j] == '\t') {
                        j += 1;
                    }
                    if at_line_start && (chars.get(j) == Some(&'=') || chars.get(j) == Some(&'.')) {
                        Some("key")
                    } else if word == "true" || word == "false" {
                        Some("kw")
                    } else {
                        None
                    }
                }
                Lang::Bash => {
                    if BASH_KW.contains(&word.as_str()) {
                        Some("kw")
                    } else if at_line_start {
                        Some("fn")
                    } else {
                        None
                    }
                }
                Lang::Sql => {
                    if SQL_KW.contains(&word.to_ascii_lowercase().as_str()) {
                        Some("kw")
                    } else if next_c == Some('(') {
                        Some("fn")
                    } else {
                        None
                    }
                }
                Lang::Json => {
                    if word == "true" || word == "false" || word == "null" {
                        Some("kw")
                    } else {
                        None
                    }
                }
                Lang::Plain => None,
            };
            match class {
                Some(cl) => push_tok(&mut out, cl, &word),
                None => escape_into(&mut out, &word),
            }
            continue;
        }

        // Operators worth a colour
        if matches!(c, '&' | '|' | '=' | '<' | '>' | '?' | '!' | '+' | '*' | '%') && lang != Lang::Bash {
            let mut buf = [0u8; 4];
            push_tok(&mut out, "op", c.encode_utf8(&mut buf));
            i += 1;
            continue;
        }

        let mut buf = [0u8; 4];
        escape_into(&mut out, c.encode_utf8(&mut buf));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tokens() {
        let html = highlight("#[derive(Model)]\nlet x = vec![1, 2]; // hi", "rust");
        assert!(html.contains("tk-attr"));
        assert!(html.contains("<span class=\"tk-mac\">vec!</span>"));
        assert!(html.contains("<span class=\"tk-com\">// hi</span>"));
        assert!(html.contains("<span class=\"tk-kw\">let</span>"));
    }

    #[test]
    fn sql_tokens() {
        let html = highlight("SELECT \"users\".\"id\" FROM \"users\" WHERE age >= $1", "sql");
        assert!(html.contains("<span class=\"tk-kw\">SELECT</span>"));
        assert!(html.contains("<span class=\"tk-var\">$1</span>"));
    }

    #[test]
    fn escapes() {
        assert_eq!(highlight("<a>", "text"), "&lt;a&gt;");
    }
}
