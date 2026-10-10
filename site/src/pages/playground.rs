//! `/playground`: build a query with form controls and see the generated SQL
//! (PostgreSQL / MySQL / SQLite) and the resulting rows.
//!
//! The UI only talks to the engine through [`run_query`]. Today that function is
//! backed by a tiny local mock (see the `mock` module at the bottom); it is meant
//! to be rewired to the real `memoire_orm` in-memory engine compiled to WASM.

use gloo::timers::callback::Timeout;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::components::icons::Icon;
use crate::dom;
use crate::highlight::highlight;
use crate::i18n::use_lang;

// ===========================================================================
// Engine seam — everything the UI needs from the query engine.
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderField {
    Name,
    Age,
}

/// What the user picked in the controls.
#[derive(Clone, PartialEq, Debug)]
pub struct PlaygroundParams {
    /// `User::AGE.ge(min_age)`
    pub min_age: i32,
    /// `User::NAME.contains(name_contains)`; empty = no name filter.
    pub name_contains: String,
    pub order_by: OrderField,
    pub descending: bool,
    pub limit: u32,
}

impl Default for PlaygroundParams {
    fn default() -> Self {
        PlaygroundParams {
            min_age: 18,
            name_contains: "li".to_string(),
            order_by: OrderField::Name,
            descending: false,
            limit: 20,
        }
    }
}

/// What the engine returns for one run.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PlaygroundOutput {
    pub sql_postgres: String,
    pub sql_mysql: String,
    pub sql_sqlite: String,
    /// `(id, name, email, age)`
    pub rows: Vec<(i64, String, String, Option<i32>)>,
    /// Execution time of the query, in microseconds.
    pub micros: f64,
}

/// `true` while [`run_query`] is backed by the mock: shows a "preview" notice.
pub const PLAYGROUND_USES_MOCK: bool = true;

/// Run the playground query. Swap the body for the real engine when it's ready.
pub fn run_query(params: &PlaygroundParams) -> PlaygroundOutput {
    mock::run(params)
}

// ===========================================================================
// UI
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Postgres,
    MySql,
    Sqlite,
}

impl Dialect {
    const ALL: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

    fn label(self) -> &'static str {
        match self {
            Dialect::Postgres => "PostgreSQL",
            Dialect::MySql => "MySQL",
            Dialect::Sqlite => "SQLite",
        }
    }

    fn placeholder(self, n: usize) -> String {
        match self {
            Dialect::Postgres => format!("${n}"),
            Dialect::MySql => "?".to_string(),
            Dialect::Sqlite => format!("?{n}"),
        }
    }
}

/// The Rust code equivalent to the current controls.
fn rust_code(p: &PlaygroundParams) -> String {
    let filter = if p.name_contains.is_empty() {
        format!("User::AGE.ge({})", p.min_age)
    } else {
        format!("User::AGE.ge({}) & User::NAME.contains({:?})", p.min_age, p.name_contains)
    };
    let col = match p.order_by {
        OrderField::Name => "NAME",
        OrderField::Age => "AGE",
    };
    let dir = if p.descending { "desc" } else { "asc" };
    format!(
        "let users = User::query()\n    .filter({filter})\n    .order_by(User::{col}.{dir}())\n    .limit({})\n    .all(&db)\n    .await?;",
        p.limit
    )
}

/// Bound parameter values, in order.
fn bound_params(p: &PlaygroundParams) -> Vec<String> {
    let mut v = vec![p.min_age.to_string()];
    if !p.name_contains.is_empty() {
        v.push(format!("'%{}%'", mock::escape_like(&p.name_contains)));
    }
    v
}

fn fmt_micros(us: f64) -> String {
    if us < 0.01 {
        "< 0.01 µs".to_string()
    } else if us < 1000.0 {
        format!("{us:.2} µs")
    } else {
        format!("{:.2} ms", us / 1000.0)
    }
}

#[component]
pub fn PlaygroundPage() -> Html {
    let t = use_lang().lang.t();
    let params = use_state(PlaygroundParams::default);
    let dialect = use_state(|| Dialect::Postgres);
    let copied = use_state(|| false);
    let copy_timer = use_mut_ref(|| None::<Timeout>);

    let output = use_memo((*params).clone(), run_query);
    let rust_html = use_memo((*params).clone(), |p| highlight(&rust_code(p), "rust"));

    let sql = match *dialect {
        Dialect::Postgres => &output.sql_postgres,
        Dialect::MySql => &output.sql_mysql,
        Dialect::Sqlite => &output.sql_sqlite,
    };
    let sql_html = highlight(sql, "sql");

    let update = |f: fn(&mut PlaygroundParams, String)| {
        let params = params.clone();
        move |value: String| {
            let mut next = (*params).clone();
            f(&mut next, value);
            params.set(next);
        }
    };

    let on_age = {
        let set = update(|p, v| p.min_age = v.parse().unwrap_or(p.min_age).clamp(0, 100));
        Callback::from(move |e: InputEvent| set(e.target_unchecked_into::<HtmlInputElement>().value()))
    };
    let on_name = {
        let set = update(|p, v| p.name_contains = v.chars().take(32).collect());
        Callback::from(move |e: InputEvent| set(e.target_unchecked_into::<HtmlInputElement>().value()))
    };
    let on_order = {
        let set = update(|p, v| {
            p.order_by = if v == "age" { OrderField::Age } else { OrderField::Name }
        });
        Callback::from(move |e: Event| set(e.target_unchecked_into::<HtmlSelectElement>().value()))
    };
    let on_dir = {
        let set = update(|p, _| p.descending = !p.descending);
        Callback::from(move |_: MouseEvent| set(String::new()))
    };
    let on_limit = {
        let set = update(|p, v| p.limit = v.parse().unwrap_or(p.limit).clamp(1, 100));
        Callback::from(move |e: InputEvent| set(e.target_unchecked_into::<HtmlInputElement>().value()))
    };
    let on_reset = {
        let params = params.clone();
        Callback::from(move |_| params.set(PlaygroundParams::default()))
    };
    let on_copy = {
        let (copied, copy_timer, sql) = (copied.clone(), copy_timer.clone(), sql.clone());
        Callback::from(move |_| {
            dom::copy_to_clipboard(&sql);
            copied.set(true);
            let copied = copied.clone();
            *copy_timer.borrow_mut() = Some(Timeout::new(1500, move || copied.set(false)));
        })
    };

    let p = &*params;
    let age_pct = format!("--pct:{}%", p.min_age);
    html! {
        <>
            <section class="page-hero container">
                <span class="eyebrow hero-anim">{ "WebAssembly" }</span>
                <h1 class="page-title hero-anim shimmer" style="--d:80ms">{ t.pg_title }</h1>
                <p class="page-subtitle hero-anim" style="--d:160ms">{ t.pg_subtitle }</p>
            </section>

            <section class="container playground hero-anim" style="--d:240ms">
                <aside class="pg-controls glass">
                    <div class="pg-head">
                        <h2>{ t.pg_controls }</h2>
                        <button type="button" class="btn btn-ghost btn-xs" onclick={on_reset}>{ t.pg_reset }</button>
                    </div>

                    <label class="field">
                        <span class="field-label">{ t.pg_min_age }<output class="field-value">{ p.min_age }</output></span>
                        <input type="range" min="0" max="100" value={p.min_age.to_string()} oninput={on_age} style={age_pct} />
                    </label>

                    <label class="field">
                        <span class="field-label">{ t.pg_name_contains }</span>
                        <input type="text" class="input" placeholder={t.pg_name_placeholder} value={p.name_contains.clone()} oninput={on_name} spellcheck="false" autocomplete="off" />
                    </label>

                    <div class="field-row">
                        <label class="field">
                            <span class="field-label">{ t.pg_order_by }</span>
                            <select class="input" onchange={on_order}>
                                <option value="name" selected={p.order_by == OrderField::Name}>{ t.pg_order_name }</option>
                                <option value="age" selected={p.order_by == OrderField::Age}>{ t.pg_order_age }</option>
                            </select>
                        </label>
                        <div class="field">
                            <span class="field-label">{ t.pg_direction }</span>
                            <button type="button" class={classes!("dir-toggle", p.descending.then_some("is-desc"))} onclick={on_dir} aria-pressed={if p.descending { "true" } else { "false" }}>
                                <span class="dir-arrow">{ Icon::ArrowUp.html() }</span>
                                { if p.descending { t.pg_desc } else { t.pg_asc } }
                            </button>
                        </div>
                    </div>

                    <label class="field">
                        <span class="field-label">{ t.pg_limit }</span>
                        <input type="number" class="input" min="1" max="100" value={p.limit.to_string()} oninput={on_limit} />
                    </label>

                    <div class="pg-rust">
                        <span class="pane-title">{ t.pg_rust }</span>
                        <pre><code>{ Html::from_html_unchecked(AttrValue::from((*rust_html).clone())) }</code></pre>
                    </div>
                </aside>

                <div class="pg-output">
                    <div class="pg-pane glass">
                        <div class="pane-head">
                            <span class="pane-title">{ t.pg_sql }</span>
                            <div class="dialect-tabs" role="tablist">
                                { for Dialect::ALL.iter().map(|&d| {
                                    let dialect = dialect.clone();
                                    let active = *dialect == d;
                                    html! {
                                        <button type="button" role="tab" aria-selected={if active { "true" } else { "false" }}
                                            class={classes!("dialect-tab", active.then_some("active"))}
                                            onclick={Callback::from(move |_| dialect.set(d))}>{ d.label() }</button>
                                    }
                                }) }
                            </div>
                            <button type="button" class={classes!("cw-copy", copied.then_some("is-copied"))} onclick={on_copy} aria-label={t.copy}>
                                { if *copied { Icon::Check.html() } else { Icon::Copy.html() } }
                            </button>
                        </div>
                        <pre class="pg-sql wrap" key={sql.clone()}><code>{ Html::from_html_unchecked(AttrValue::from(sql_html)) }</code></pre>
                        <div class="pg-params">
                            <span class="pane-subtitle">{ t.pg_params }</span>
                            { for bound_params(p).into_iter().enumerate().map(|(i, v)| html! {
                                <span class="param-chip"><code>{ dialect.placeholder(i + 1) }</code>{ " = " }<code>{ v }</code></span>
                            }) }
                        </div>
                    </div>

                    <div class="pg-pane glass">
                        <div class="pane-head">
                            <span class="pane-title">{ t.pg_result }</span>
                            <span class="row-count">{ format!("{} {}", output.rows.len(), t.pg_rows) }</span>
                            <span class="exec-badge" key={format!("{:.4}", output.micros)}>
                                { Icon::Zap.html() }{ format!("{} {}", t.pg_exec, fmt_micros(output.micros)) }
                            </span>
                        </div>
                        if output.rows.is_empty() {
                            <p class="pg-empty">{ t.pg_no_rows }</p>
                        } else {
                            <div class="table-wrap">
                                <table class="pg-table">
                                    <thead><tr><th>{ "id" }</th><th>{ "name" }</th><th>{ "email" }</th><th>{ "age" }</th></tr></thead>
                                    <tbody>
                                        { for output.rows.iter().enumerate().map(|(i, (id, name, email, age))| html! {
                                            <tr key={*id} style={format!("--i:{i}")}>
                                                <td class="num">{ id }</td>
                                                <td>{ name }</td>
                                                <td class="muted">{ email }</td>
                                                <td class="num">{ age.map(|a| a.to_string()).unwrap_or_else(|| "NULL".into()) }</td>
                                            </tr>
                                        }) }
                                    </tbody>
                                </table>
                            </div>
                        }
                    </div>

                    if PLAYGROUND_USES_MOCK {
                        <p class="pg-note">{ Icon::Cpu.html() }<span>{ t.pg_mock_note }</span></p>
                    }
                </div>
            </section>
        </>
    }
}

// ===========================================================================
// Mock engine — to be replaced by the real memoire_orm MemoryDb (see run_query).
// ===========================================================================

mod mock {
    use super::{OrderField, PlaygroundOutput, PlaygroundParams};
    use crate::dom;

    const USERS: &[(i64, &str, &str, Option<i32>)] = &[
        (1, "Alice", "alice@rust.dev", Some(31)),
        (2, "Bob", "bob@rust.dev", Some(17)),
        (3, "Charlie", "charlie@rust.dev", Some(42)),
        (4, "Diana", "diana@rust.dev", Some(28)),
        (5, "Élodie", "elodie@rust.dev", Some(35)),
        (6, "Fernando", "fernando@rust.dev", None),
        (7, "Grace", "grace@rust.dev", Some(54)),
        (8, "Hugo", "hugo@rust.dev", Some(23)),
        (9, "Inès", "ines@rust.dev", Some(19)),
        (10, "Jules", "jules@rust.dev", Some(16)),
        (11, "Karim", "karim@rust.dev", Some(38)),
        (12, "Lucía", "lucia@rust.dev", Some(26)),
        (13, "Mateo", "mateo@rust.dev", Some(33)),
        (14, "Nora", "nora@rust.dev", None),
        (15, "Oliver", "oliver@rust.dev", Some(47)),
        (16, "Camille", "camille@rust.dev", Some(29)),
        (17, "Emilio", "emilio@rust.dev", Some(61)),
        (18, "Amelia", "amelia@rust.dev", Some(21)),
    ];

    /// Escape LIKE wildcards with a backslash (matches `ESCAPE '\'`).
    pub fn escape_like(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if matches!(c, '%' | '_' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    fn sql(p: &PlaygroundParams, q: fn(&str) -> String, ph: fn(usize) -> String, escape: &str) -> String {
        let col = |c: &str| format!("{}.{}", q("users"), q(c));
        let cols = ["id", "email", "name", "age"].map(col).join(", ");
        let age = format!("{} >= {}", col("age"), ph(1));
        let filter = if p.name_contains.is_empty() {
            age
        } else {
            format!("({age} AND {} LIKE {} ESCAPE {escape})", col("name"), ph(2))
        };
        let order = match p.order_by {
            OrderField::Name => col("name"),
            OrderField::Age => col("age"),
        };
        let dir = if p.descending { "DESC" } else { "ASC" };
        format!(
            "SELECT {cols} FROM {} WHERE {filter} ORDER BY {order} {dir} LIMIT {}",
            q("users"),
            p.limit
        )
    }

    fn execute(p: &PlaygroundParams) -> Vec<(i64, String, String, Option<i32>)> {
        let mut rows: Vec<&(i64, &str, &str, Option<i32>)> = USERS
            .iter()
            .filter(|(_, name, _, age)| {
                age.is_some_and(|a| a >= p.min_age)
                    && (p.name_contains.is_empty() || name.contains(p.name_contains.as_str()))
            })
            .collect();
        match p.order_by {
            OrderField::Name => rows.sort_by(|a, b| a.1.cmp(b.1)),
            OrderField::Age => rows.sort_by_key(|r| r.3),
        }
        if p.descending {
            rows.reverse();
        }
        rows.into_iter()
            .take(p.limit as usize)
            .map(|(id, name, email, age)| (*id, name.to_string(), email.to_string(), *age))
            .collect()
    }

    pub fn run(p: &PlaygroundParams) -> PlaygroundOutput {
        // Average over several runs: browsers round performance.now() quite coarsely.
        const RUNS: u32 = 200;
        let start = dom::now_ms();
        let mut rows = Vec::new();
        for _ in 0..RUNS {
            rows = std::hint::black_box(execute(std::hint::black_box(p)));
        }
        let micros = (dom::now_ms() - start) * 1000.0 / f64::from(RUNS);
        PlaygroundOutput {
            sql_postgres: sql(p, |s| format!("\"{s}\""), |n| format!("${n}"), "'\\'"),
            sql_mysql: sql(p, |s| format!("`{s}`"), |_| "?".to_string(), "'\\\\'"),
            sql_sqlite: sql(p, |s| format!("\"{s}\""), |n| format!("?{n}"), "'\\'"),
            rows,
            micros,
        }
    }
}
