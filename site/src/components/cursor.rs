//! Glowing cursor follower (desktop only): a dot that sticks to the pointer and a
//! ring that eases behind it and grows over interactive elements.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, PointerEvent};
use yew::prelude::*;

use crate::dom::{self, RafLoop};

const INTERACTIVE: &str = "a, button, input, select, textarea, label, [role=tab], .tilt";

#[component]
pub fn CursorFollower() -> Html {
    let dot = use_node_ref();
    let ring = use_node_ref();
    {
        let (dot, ring) = (dot.clone(), ring.clone());
        use_effect_with((), move |_| {
            let mut listeners: Vec<EventListener> = Vec::new();
            let raf: Rc<RefCell<Option<RafLoop>>> = Rc::default();
            let active = dom::has_fine_pointer() && !dom::prefers_reduced_motion();
            if active {
                // CSS only shows the follower when <body> has this class.
                dom::set_body_class("has-cursor", true);
            }
            if let (true, Some(dot), Some(ring)) = (active, dot.cast::<HtmlElement>(), ring.cast::<HtmlElement>()) {
                let target = Rc::new(Cell::new((-100.0f64, -100.0f64)));
                let pos = Rc::new(Cell::new((-100.0f64, -100.0f64)));
                let running = Rc::new(Cell::new(false));
                {
                    let (raf, ring, target, pos, running) = (raf.clone(), ring.clone(), target.clone(), pos.clone(), running.clone());
                    let dot = dot.clone();
                    listeners.push(EventListener::new(&dom::document(), "pointermove", move |e| {
                        let Some(e) = e.dyn_ref::<PointerEvent>() else { return };
                        if e.pointer_type() != "mouse" {
                            return;
                        }
                        let (x, y) = (f64::from(e.client_x()), f64::from(e.client_y()));
                        target.set((x, y));
                        if pos.get().0 < -50.0 {
                            pos.set((x, y));
                        }
                        dom::set_css_var(&dot, "transform", &format!("translate3d({x:.1}px,{y:.1}px,0)"));
                        let hovering = e
                            .target()
                            .and_then(|t| t.dyn_into::<Element>().ok())
                            .and_then(|t| t.closest(INTERACTIVE).ok().flatten())
                            .is_some();
                        let _ = ring.class_list().toggle_with_force("is-hover", hovering);
                        let _ = ring.class_list().add_1("is-visible");
                        let _ = dot.class_list().add_1("is-visible");
                        if !running.get() {
                            running.set(true);
                            let (ring, target, pos, running) = (ring.clone(), target.clone(), pos.clone(), running.clone());
                            *raf.borrow_mut() = Some(RafLoop::start(move |_| {
                                let (tx, ty) = target.get();
                                let (px, py) = pos.get();
                                let (nx, ny) = (px + (tx - px) * 0.18, py + (ty - py) * 0.18);
                                pos.set((nx, ny));
                                dom::set_css_var(&ring, "transform", &format!("translate3d({nx:.1}px,{ny:.1}px,0)"));
                                let moving = (tx - nx).abs() > 0.2 || (ty - ny).abs() > 0.2;
                                running.set(moving);
                                moving
                            }));
                        }
                    }));
                }
                {
                    let (dot, ring) = (dot.clone(), ring.clone());
                    listeners.push(EventListener::new(&dom::document(), "mouseleave", move |_| {
                        let _ = dot.class_list().remove_1("is-visible");
                        let _ = ring.class_list().remove_1("is-visible");
                    }));
                }
                {
                    let down = ring.clone();
                    listeners.push(EventListener::new(&dom::document(), "pointerdown", move |_| {
                        let _ = down.class_list().add_1("is-down");
                    }));
                    listeners.push(EventListener::new(&dom::document(), "pointerup", move |_| {
                        let _ = ring.class_list().remove_1("is-down");
                    }));
                }
            }
            move || {
                drop(listeners);
                raf.borrow_mut().take();
                dom::set_body_class("has-cursor", false);
            }
        });
    }
    html! {
        <>
            <div ref={ring} class="cursor-ring" aria-hidden="true"></div>
            <div ref={dot} class="cursor-dot" aria-hidden="true"></div>
        </>
    }
}
