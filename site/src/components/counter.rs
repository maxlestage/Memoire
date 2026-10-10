//! Number that counts up (ease-out) the first time it scrolls into view.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use web_sys::Element;
use yew::prelude::*;

use crate::dom::{self, RafLoop};

#[derive(Properties, PartialEq)]
pub struct CounterProps {
    pub value: f64,
    #[prop_or_default]
    pub decimals: u8,
    #[prop_or_default]
    pub prefix: AttrValue,
    #[prop_or_default]
    pub suffix: AttrValue,
    #[prop_or(1800)]
    pub duration: u32,
}

#[component]
pub fn Counter(props: &CounterProps) -> Html {
    let node = use_node_ref();
    // Prerendered first view: the final value, without counting up.
    let settled = *use_state(dom::settled);
    let value = props.value;
    let shown = use_state(move || if settled { value } else { 0.0 });
    {
        let node = node.clone();
        let shown = shown.clone();
        let duration = f64::from(props.duration.max(1));
        use_effect_with(props.value, move |&value| {
            let raf: Rc<RefCell<Option<RafLoop>>> = Rc::default();
            let reduced = settled || dom::prefers_reduced_motion();
            if reduced {
                // No animation: show the final number right away.
                shown.set(value);
            }
            let io = node.cast::<Element>().filter(|_| !reduced).and_then(|el| {
                let raf = raf.clone();
                dom::observe_once(&el, 0.4, move || {
                    let start = Cell::new(None::<f64>);
                    *raf.borrow_mut() = Some(RafLoop::start(move |ts| {
                        let t0 = start.get().unwrap_or(ts);
                        start.set(Some(t0));
                        let p = ((ts - t0) / duration).clamp(0.0, 1.0);
                        let eased = 1.0 - (1.0 - p).powi(3);
                        shown.set(value * eased);
                        p < 1.0
                    }));
                })
            });
            move || {
                drop(io);
                drop(raf);
            }
        });
    }
    let text = format!(
        "{}{:.*}{}",
        props.prefix,
        usize::from(props.decimals),
        *shown,
        props.suffix
    );
    html! { <span ref={node} class="counter">{ text }</span> }
}
