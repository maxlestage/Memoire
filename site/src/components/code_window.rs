//! Animated code window (tabs: Model / Query / Transaction / Migration) and a fake
//! terminal that "types" the matching output when it scrolls into view.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo::timers::callback::{Interval, Timeout};
use web_sys::Element;
use yew::prelude::*;

use super::icons::Icon;
use crate::data::samples::{self, TermLine};
use crate::dom;
use crate::highlight::highlight;
use crate::i18n::use_lang;

const FILES: [&str; 4] = ["models/user.rs", "src/queries.rs", "src/transaction.rs", "src/migrate.rs"];
const CODES: [&str; 4] = [samples::MODEL, samples::QUERY, samples::TRANSACTION, samples::MIGRATION];

#[component]
pub fn CodeShowcase() -> Html {
    let ctx = use_lang();
    let t = ctx.lang.t();
    let tab = use_state(|| 0usize);
    let copied = use_state(|| false);
    let copy_timer = use_mut_ref(|| None::<Timeout>);

    let code_html = use_memo(*tab, |&tab| highlight(CODES[tab], "rust"));
    let lines = CODES[*tab].lines().count();

    let on_copy = {
        let (tab, copied, copy_timer) = (tab.clone(), copied.clone(), copy_timer.clone());
        Callback::from(move |_| {
            dom::copy_to_clipboard(CODES[*tab]);
            copied.set(true);
            let copied = copied.clone();
            *copy_timer.borrow_mut() = Some(Timeout::new(1600, move || copied.set(false)));
        })
    };

    html! {
        <div class="showcase">
            <div class="code-window">
                <div class="cw-head">
                    <span class="cw-dots" aria-hidden="true"><i></i><i></i><i></i></span>
                    <div class="cw-tabs" role="tablist">
                        { for t.code_tabs.iter().enumerate().map(|(i, name)| {
                            let tab = tab.clone();
                            let active = *tab == i;
                            html! {
                                <button
                                    type="button"
                                    role="tab"
                                    aria-selected={if active { "true" } else { "false" }}
                                    class={classes!("cw-tab", active.then_some("active"))}
                                    onclick={Callback::from(move |_| tab.set(i))}
                                >{ *name }</button>
                            }
                        }) }
                    </div>
                    <button type="button" class={classes!("cw-copy", copied.then_some("is-copied"))} onclick={on_copy} aria-label={t.copy}>
                        { if *copied { Icon::Check.html() } else { Icon::Copy.html() } }
                    </button>
                </div>
                <div class="cw-file">{ FILES[*tab] }</div>
                <div class="cw-body" key={*tab}>
                    <pre class="cw-gutter" aria-hidden="true">{ (1..=lines).map(|n| format!("{n}\n")).collect::<String>() }</pre>
                    <pre class="cw-code"><code>{ Html::from_html_unchecked(AttrValue::from((*code_html).clone())) }</code></pre>
                </div>
            </div>
            <Terminal key={*tab} script={samples::TERMINAL[*tab]} title={t.terminal} />
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct TerminalProps {
    pub script: &'static [TermLine],
    pub title: AttrValue,
}

/// Progress through the script: (completed lines, chars typed in the current line).
#[component]
pub fn Terminal(props: &TerminalProps) -> Html {
    let node = use_node_ref();
    // Prerendered first view: the whole script, already typed.
    let settled = *use_state(dom::settled);
    let script_len = props.script.len();
    let progress = use_state_eq(move || if settled { (script_len, 0) } else { (0usize, 0usize) });
    {
        let (node, progress) = (node.clone(), progress.clone());
        let script = props.script;
        use_effect_with((), move |_| {
            let interval: Rc<RefCell<Option<Interval>>> = Rc::default();
            let io = node.cast::<Element>().filter(|_| !settled).and_then(|el| {
                let interval = interval.clone();
                dom::observe_once(&el, 0.3, move || {
                    if dom::prefers_reduced_motion() {
                        progress.set((script.len(), 0));
                        return;
                    }
                    let line = Cell::new(0usize);
                    let ch = Cell::new(0usize);
                    let wait = Cell::new(6u32);
                    *interval.borrow_mut() = Some(Interval::new(32, move || {
                        if wait.get() > 0 {
                            wait.set(wait.get() - 1);
                            return;
                        }
                        let Some(current) = script.get(line.get()) else { return };
                        match current {
                            TermLine::Cmd(cmd) => {
                                if ch.get() < cmd.chars().count() {
                                    ch.set(ch.get() + 1);
                                } else {
                                    line.set(line.get() + 1);
                                    ch.set(0);
                                    wait.set(10);
                                }
                            }
                            TermLine::Out(_) | TermLine::Ok(_) => {
                                line.set(line.get() + 1);
                                wait.set(3);
                            }
                        }
                        progress.set((line.get(), ch.get()));
                    }));
                })
            });
            move || {
                drop(io);
                interval.borrow_mut().take();
            }
        });
    }

    let (done, typed) = *progress;
    let render_line = |l: &TermLine, partial: Option<usize>| match *l {
        TermLine::Cmd(cmd) => {
            let text: String = match partial {
                Some(n) => cmd.chars().take(n).collect(),
                None => cmd.to_string(),
            };
            html! {
                <div class="term-line cmd">
                    <span class="term-prompt">{ "❯ " }</span>{ text }
                    if partial.is_some() { <span class="term-caret"></span> }
                </div>
            }
        }
        TermLine::Out(out) => html! { <div class="term-line out">{ out }</div> },
        TermLine::Ok(out) => html! { <div class="term-line ok">{ out }</div> },
    };
    let finished = done >= props.script.len();

    html! {
        <div ref={node} class="terminal" aria-label={props.title.clone()}>
            <div class="term-head">
                <span class="cw-dots" aria-hidden="true"><i></i><i></i><i></i></span>
                <span class="term-title">{ Icon::Terminal.html() }{ props.title.clone() }</span>
            </div>
            <div class="term-body">
                { for props.script.iter().take(done).map(|l| render_line(l, None)) }
                if let Some(cur) = props.script.get(done) {
                    if matches!(cur, TermLine::Cmd(_)) { { render_line(cur, Some(typed)) } }
                }
                if finished {
                    <div class="term-line cmd"><span class="term-prompt">{ "❯ " }</span><span class="term-caret"></span></div>
                }
            </div>
        </div>
    }
}
