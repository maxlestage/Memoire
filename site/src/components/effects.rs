//! Global pointer effects, driven by one delegated `pointermove` listener:
//! * `.tilt`      → 3D tilt (`--rx`, `--ry`)
//! * `.spotlight` → glow following the cursor (`--mx`, `--my`)
//! * `.magnetic`  → button gently attracted by the cursor (`--tx`, `--ty`)
//!
//! Disabled on touch devices and when the user prefers reduced motion.

use std::cell::RefCell;
use std::rc::Rc;

use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, PointerEvent};
use yew::prelude::*;

use crate::dom;

type Slot = Rc<RefCell<Option<HtmlElement>>>;

fn closest(target: &Element, selector: &str) -> Option<HtmlElement> {
    target
        .closest(selector)
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<HtmlElement>().ok())
}

/// Remember the hovered element; reset the previous one when it changes.
fn swap(slot: &Slot, next: Option<&HtmlElement>, reset: &[&str]) {
    let mut cur = slot.borrow_mut();
    let same = matches!((cur.as_ref(), next), (Some(a), Some(b)) if a == b);
    if !same {
        if let Some(prev) = cur.take() {
            for prop in reset {
                let _ = prev.style().remove_property(prop);
            }
        }
        *cur = next.cloned();
    }
}

#[hook]
pub fn use_pointer_effects() {
    use_effect_with((), |_| {
        let listener = (dom::has_fine_pointer() && !dom::prefers_reduced_motion()).then(|| {
            let tilt: Slot = Rc::default();
            let magnet: Slot = Rc::default();
            EventListener::new(&dom::document(), "pointermove", move |event| {
                let Some(e) = event.dyn_ref::<PointerEvent>() else { return };
                if e.pointer_type() != "mouse" {
                    return;
                }
                let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                    return;
                };
                let (cx, cy) = (f64::from(e.client_x()), f64::from(e.client_y()));

                if let Some(el) = closest(&target, ".spotlight") {
                    let r = el.get_bounding_client_rect();
                    dom::set_css_var(&el, "--mx", &format!("{:.1}px", cx - r.left()));
                    dom::set_css_var(&el, "--my", &format!("{:.1}px", cy - r.top()));
                }

                let tilt_el = closest(&target, ".tilt");
                if let Some(el) = &tilt_el {
                    let r = el.get_bounding_client_rect();
                    let x = ((cx - r.left()) / r.width().max(1.0)).clamp(0.0, 1.0);
                    let y = ((cy - r.top()) / r.height().max(1.0)).clamp(0.0, 1.0);
                    dom::set_css_var(el, "--rx", &format!("{:.2}deg", (0.5 - y) * 9.0));
                    dom::set_css_var(el, "--ry", &format!("{:.2}deg", (x - 0.5) * 11.0));
                }
                swap(&tilt, tilt_el.as_ref(), &["--rx", "--ry"]);

                let mag_el = closest(&target, ".magnetic");
                if let Some(el) = &mag_el {
                    let r = el.get_bounding_client_rect();
                    let dx = cx - (r.left() + r.width() / 2.0);
                    let dy = cy - (r.top() + r.height() / 2.0);
                    dom::set_css_var(el, "--tx", &format!("{:.1}px", dx * 0.22));
                    dom::set_css_var(el, "--ty", &format!("{:.1}px", dy * 0.3));
                }
                swap(&magnet, mag_el.as_ref(), &["--tx", "--ty"]);
            })
        });
        move || drop(listener)
    });
}
