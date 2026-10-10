//! Rotating tagline that types, pauses, deletes, and moves to the next line.

use std::cell::Cell;
use std::rc::Rc;

use gloo::timers::callback::Interval;
use yew::prelude::*;

use crate::dom;

#[derive(Properties, PartialEq)]
pub struct TypingProps {
    pub lines: &'static [&'static str],
}

#[derive(Clone, Copy)]
enum Phase {
    Typing,
    Holding(u32),
    Deleting,
}

#[component]
pub fn Typing(props: &TypingProps) -> Html {
    let text = use_state_eq(String::new);
    {
        let text = text.clone();
        use_effect_with(props.lines, move |&lines| {
            let interval = if lines.is_empty() {
                None
            } else if dom::prefers_reduced_motion() {
                text.set(lines[0].to_string());
                None
            } else {
                let line = Rc::new(Cell::new(0usize));
                let chars = Rc::new(Cell::new(0usize));
                let phase = Rc::new(Cell::new(Phase::Typing));
                Some(Interval::new(48, move || {
                    let current: Vec<char> = lines[line.get() % lines.len()].chars().collect();
                    match phase.get() {
                        Phase::Typing => {
                            if chars.get() < current.len() {
                                chars.set(chars.get() + 1);
                            } else {
                                phase.set(Phase::Holding(38));
                            }
                        }
                        Phase::Holding(0) => phase.set(Phase::Deleting),
                        Phase::Holding(n) => phase.set(Phase::Holding(n - 1)),
                        Phase::Deleting => {
                            let c = chars.get();
                            if c > 0 {
                                chars.set(c.saturating_sub(2));
                            } else {
                                line.set(line.get() + 1);
                                phase.set(Phase::Typing);
                            }
                        }
                    }
                    text.set(current.iter().take(chars.get()).collect());
                }))
            };
            move || drop(interval)
        });
    }
    html! {
        <span class="typing">
            <span class="sr-only">{ props.lines.join(" ") }</span>
            <span class="typing-text" aria-hidden="true">{ (*text).clone() }</span>
            <span class="typing-caret" aria-hidden="true"></span>
        </span>
    }
}
