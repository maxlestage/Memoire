//! Page motion and the small behaviors of the static parts of the page: theme, language
//! memory, navbar and scroll progress, mobile menu, scroll reveals, counters, copy buttons,
//! the docs table of contents, and, with a mouse, the cursor follower and the tilt,
//! spotlight and magnetic effects. Pure CSS does the rest (entrance animations, marquee,
//! page transitions).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, KeyboardEvent, MouseEvent, PointerEvent, ScrollBehavior, ScrollToOptions};

use super::{
    all, document, fine_pointer, observe, on, on_passive, reduced_motion, request_frame, seen, set_body_class, set_var,
    viewport_height, when_visible, window,
};
use crate::i18n::t;
use crate::routes::{Lang, base, link};
use crate::{THEME_DARK, THEME_LIGHT};

pub fn init(root: &Element) {
    remember_language(root);
    scroll_state();
    mobile_menu();
    to_top();
    reveal(root);
    counters();
    copy_buttons(root);
    toc();
    super::particles::init();
    if fine_pointer() && !reduced_motion() {
        cursor();
        pointer_effects();
    }
}

// ---------------------------------------------------------------------------
// Preferences: theme and language
// ---------------------------------------------------------------------------

fn storage_get(key: &str) -> Option<String> {
    window().local_storage().ok().flatten()?.get_item(key).ok().flatten()
}

fn storage_set(key: &str, value: &str) {
    if let Ok(Some(storage)) = window().local_storage() {
        let _ = storage.set_item(key, value);
    }
}

/// Sets a cookie for the whole site (the server reads it to render the right theme and
/// to pick the language of `/`).
fn set_cookie(name: &str, value: &str) {
    if let Ok(doc) = document().dyn_into::<web_sys::HtmlDocument>() {
        let _ = doc.set_cookie(&format!("{name}={value}; path={}/; max-age=31536000; SameSite=Lax", base()));
    }
}

fn cookie(name: &str) -> Option<String> {
    let doc = document().dyn_into::<web_sys::HtmlDocument>().ok()?;
    let cookies = doc.cookie().ok()?;
    cookies.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

/// The theme: the visitor's choice (kept in `localStorage` and a cookie), else the
/// system's. The toggle buttons switch it.
pub fn theme(root: &Element) {
    let chosen = storage_get("memoire-theme").filter(|t| t == "light" || t == "dark");
    let theme = chosen
        .clone()
        .or_else(|| root.get_attribute("data-theme"))
        .unwrap_or_else(|| if super::media("(prefers-color-scheme: light)") { "light" } else { "dark" }.to_string());
    apply_theme(root, &theme);
    if let Some(chosen) = chosen {
        set_cookie("memoire-theme", &chosen);
    }
    let root = root.clone();
    on(&document(), "click", move |e: MouseEvent| {
        let toggled = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|t| t.closest(".theme-toggle").ok().flatten());
        if toggled.is_some() {
            let next = if root.get_attribute("data-theme").as_deref() == Some("light") { "dark" } else { "light" };
            apply_theme(&root, next);
            storage_set("memoire-theme", next);
            set_cookie("memoire-theme", next);
        }
    });
}

fn apply_theme(root: &Element, theme: &str) {
    let _ = root.set_attribute("data-theme", theme);
    if let Ok(Some(meta)) = document().query_selector(r#"meta[name="theme-color"]"#) {
        let _ = meta.set_attribute("content", if theme == "light" { THEME_LIGHT } else { THEME_DARK });
    }
}

/// The language of the page becomes the visitor's: `/` opens in it next time.
fn remember_language(root: &Element) {
    if let Some(lang) = root.get_attribute("lang").as_deref().and_then(Lang::parse) {
        set_cookie("memoire-lang", lang.code());
        storage_set("memoire-lang", lang.code());
    }
}

/// Goes to `target` (`/`, `/docs/models/`) in the visitor's language: the last one read,
/// else the browser's, else English.
pub fn redirect(target: &str) {
    let navigator = window().navigator();
    let lang = cookie("memoire-lang")
        .or_else(|| storage_get("memoire-lang"))
        .and_then(|l| Lang::parse(&l))
        .or_else(|| navigator.languages().iter().filter_map(|l| l.as_string()).find_map(|l| Lang::parse(&l)))
        .unwrap_or(Lang::En);
    let location = window().location();
    let rest = format!("{}{}", location.search().unwrap_or_default(), location.hash().unwrap_or_default());
    let _ = location.replace(&format!("{}{rest}", link(&format!("/{}{target}", lang.code()))));
}

// ---------------------------------------------------------------------------
// Navbar, progress, back to top, mobile menu
// ---------------------------------------------------------------------------

fn scroll_progress() -> f64 {
    let max = document().document_element().map_or(1.0, |e| f64::from(e.scroll_height()) - viewport_height());
    if max <= 0.0 { 0.0 } else { (window().scroll_y().unwrap_or(0.0) / max).clamp(0.0, 1.0) }
}

/// The navbar turns to glass once scrolled; the progress bar, the back-to-top ring and
/// the background parallax follow the scroll (one update per frame).
fn scroll_state() {
    let navbar = all(".navbar");
    let progress = all(".scroll-progress");
    let to_top = all(".to-top");
    let background = if reduced_motion() { Vec::new() } else { all(".bg") };
    let update = move || {
        let y = window().scroll_y().unwrap_or(0.0);
        let p = scroll_progress();
        for el in &navbar {
            let _ = el.class_list().toggle_with_force("scrolled", y > 12.0);
        }
        for el in &progress {
            set_var(el, "--p", &format!("{p:.4}"));
        }
        for el in &to_top {
            let _ = el.class_list().toggle_with_force("is-visible", y > 640.0);
            set_var(el, "--p", &format!("{p:.3}"));
        }
        for el in &background {
            set_var(el, "--sy", &format!("{y:.0}"));
        }
    };
    update();
    let update = Rc::new(update);
    let ticking = Rc::new(Cell::new(false));
    on_passive(&window(), "scroll", move |_: web_sys::Event| {
        if ticking.replace(true) {
            return;
        }
        let (update, ticking) = (update.clone(), ticking.clone());
        request_frame(move |_| {
            ticking.set(false);
            update();
            false
        });
    });
}

fn set_menu(open: bool) {
    for el in all(".navbar") {
        let _ = el.class_list().toggle_with_force("menu-is-open", open);
    }
    for burger in all(".burger") {
        let _ = burger.class_list().toggle_with_force("is-open", open);
        let _ = burger.set_attribute("aria-expanded", if open { "true" } else { "false" });
        let label = burger.get_attribute(if open { "data-close-label" } else { "data-open-label" });
        if let Some(label) = label {
            let _ = burger.set_attribute("aria-label", &label);
        }
    }
    for menu in all(".mobile-menu") {
        let _ = menu.class_list().toggle_with_force("is-open", open);
        if open {
            let _ = menu.remove_attribute("inert");
        } else {
            let _ = menu.set_attribute("inert", "");
        }
    }
    set_body_class("menu-open", open);
}

/// The burger opens the full-screen menu; Escape or a link closes it.
fn mobile_menu() {
    let open = Rc::new(Cell::new(false));
    for burger in all(".burger") {
        let open = open.clone();
        on(&burger, "click", move |_: MouseEvent| {
            open.set(!open.get());
            set_menu(open.get());
        });
    }
    {
        let open = open.clone();
        on(&document(), "keydown", move |e: KeyboardEvent| {
            if e.key() == "Escape" && open.replace(false) {
                set_menu(false);
            }
        });
    }
    for link in all(".mobile-menu a") {
        let open = open.clone();
        on(&link, "click", move |_: MouseEvent| {
            open.set(false);
            set_menu(false);
        });
    }
}

fn scroll_to_top() {
    let options = ScrollToOptions::new();
    options.set_top(0.0);
    options.set_behavior(if reduced_motion() { ScrollBehavior::Instant } else { ScrollBehavior::Smooth });
    window().scroll_to_with_scroll_to_options(&options);
}

fn to_top() {
    for button in all("[data-to-top]") {
        on(&button, "click", |_: MouseEvent| scroll_to_top());
    }
}

// ---------------------------------------------------------------------------
// Scroll reveals and counters
// ---------------------------------------------------------------------------

/// `.reveal` blocks fade, scale or slide in when they scroll into view. Without the
/// browser bundle everything is simply shown; blocks already on screen (or above it) when
/// it starts stay as they are (no flash), the ones below are hidden (`html.motion`) until
/// they come into view.
fn reveal(root: &Element) {
    let blocks = all(".reveal");
    if reduced_motion() {
        for el in &blocks {
            let _ = el.class_list().add_1("is-visible");
        }
        return;
    }
    let hidden: Vec<HtmlElement> = blocks
        .into_iter()
        .filter(|el| {
            let seen = seen(el);
            if seen {
                let _ = el.class_list().add_1("is-visible");
            }
            !seen
        })
        .collect();
    let _ = root.class_list().add_1("motion");
    observe(&hidden, 0.12, "0px 0px -6% 0px", |entry, io| {
        if entry.is_intersecting() {
            let target = entry.target();
            let _ = target.class_list().add_1("is-visible");
            io.unobserve(&target);
        }
    });
}

/// Numbers count up (ease-out) the first time they scroll into view.
fn counters() {
    if reduced_motion() {
        return;
    }
    for el in all(".counter[data-count]") {
        let value: f64 = el.get_attribute("data-count").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let suffix = el.get_attribute("data-suffix").unwrap_or_default();
        if seen(&el) {
            continue;
        }
        el.set_text_content(Some(&format!("0{suffix}")));
        let target = el.clone();
        when_visible(&el, 0.4, move || {
            let start = Cell::new(None::<f64>);
            request_frame(move |now| {
                let t0 = start.get().unwrap_or(now);
                start.set(Some(t0));
                let p = ((now - t0) / 1800.0).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - p).powi(3);
                target.set_text_content(Some(&format!("{:.0}{suffix}", value * eased)));
                p < 1.0
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Copy buttons
// ---------------------------------------------------------------------------

/// `[data-copy]` buttons (install command) and the copy buttons of documentation code
/// blocks.
fn copy_buttons(root: &Element) {
    let lang = root.get_attribute("lang").as_deref().and_then(Lang::parse).unwrap_or(Lang::En);
    on(&document(), "click", move |e: MouseEvent| {
        let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        if let Some(button) = target.closest("[data-copy]").ok().flatten() {
            crate::web::copy_text(&button.get_attribute("data-copy").unwrap_or_default());
            let _ = button.class_list().add_1("is-copied");
            let tip = button.query_selector(".copy-tip").ok().flatten();
            if let Some(tip) = &tip {
                tip.set_text_content(button.get_attribute("data-copied").as_deref());
            }
            crate::web::after(1800, move || {
                let _ = button.class_list().remove_1("is-copied");
                if let Some(tip) = tip {
                    tip.set_text_content(Some(""));
                }
            });
            return;
        }
        if let Some(button) = target.closest(".code-copy").ok().flatten() {
            let code = button
                .closest(".code-block")
                .ok()
                .flatten()
                .and_then(|block| block.query_selector("code").ok().flatten())
                .and_then(|code| code.text_content());
            crate::web::copy_text(&code.unwrap_or_default());
            let label = button.text_content().unwrap_or_default();
            button.set_text_content(Some(t(lang).copied));
            let _ = button.class_list().add_1("is-copied");
            crate::web::after(1500, move || {
                button.set_text_content(Some(&label));
                let _ = button.class_list().remove_1("is-copied");
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Documentation: "On this page"
// ---------------------------------------------------------------------------

/// Highlights the section being read in the table of contents.
fn toc() {
    let links = all(".toc a[data-toc]");
    if links.is_empty() {
        return;
    }
    let entries: Vec<(HtmlElement, Option<Element>)> = links
        .into_iter()
        .map(|a| {
            let heading = a.get_attribute("data-toc").and_then(|id| document().get_element_by_id(&id));
            (a, heading)
        })
        .collect();
    let update = move || {
        let mut current = None;
        for (i, (_, heading)) in entries.iter().enumerate() {
            match heading {
                Some(h) if h.get_bounding_client_rect().top() < 160.0 => current = Some(i),
                Some(_) => break,
                None => {}
            }
        }
        for (i, (a, _)) in entries.iter().enumerate() {
            if let Some(li) = a.parent_element() {
                let _ = li.class_list().toggle_with_force("active", Some(i) == current);
            }
        }
    };
    update();
    let update = Rc::new(update);
    let ticking = Rc::new(Cell::new(false));
    on_passive(&window(), "scroll", move |_: web_sys::Event| {
        if ticking.replace(true) {
            return;
        }
        let (update, ticking) = (update.clone(), ticking.clone());
        request_frame(move |_| {
            ticking.set(false);
            update();
            false
        });
    });
}

// ---------------------------------------------------------------------------
// With a mouse: cursor follower, tilt, spotlight, magnetic buttons, parallax
// ---------------------------------------------------------------------------

const INTERACTIVE: &str = "a, button, input, select, textarea, label, [role=tab], .tilt";

/// A dot that sticks to the pointer and a ring that eases behind it, growing over
/// interactive elements.
fn cursor() {
    let (Some(dot), Some(ring)) = (all(".cursor-dot").pop(), all(".cursor-ring").pop()) else { return };
    set_body_class("has-cursor", true);
    let target = Rc::new(Cell::new((-100.0f64, -100.0f64)));
    let pos = Rc::new(Cell::new((-100.0f64, -100.0f64)));
    let running = Rc::new(Cell::new(false));
    {
        let (dot, ring) = (dot.clone(), ring.clone());
        on_passive(&document(), "pointermove", move |e: PointerEvent| {
            if e.pointer_type() != "mouse" {
                return;
            }
            let (x, y) = (f64::from(e.client_x()), f64::from(e.client_y()));
            target.set((x, y));
            if pos.get().0 < -50.0 {
                pos.set((x, y));
            }
            set_var(&dot, "transform", &format!("translate3d({x:.1}px,{y:.1}px,0)"));
            let hovering = e
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|t| t.closest(INTERACTIVE).ok().flatten())
                .is_some();
            let _ = ring.class_list().toggle_with_force("is-hover", hovering);
            let _ = ring.class_list().add_1("is-visible");
            let _ = dot.class_list().add_1("is-visible");
            if !running.replace(true) {
                let (ring, target, pos, running) = (ring.clone(), target.clone(), pos.clone(), running.clone());
                request_frame(move |_| {
                    let (tx, ty) = target.get();
                    let (px, py) = pos.get();
                    let (nx, ny) = (px + (tx - px) * 0.18, py + (ty - py) * 0.18);
                    pos.set((nx, ny));
                    set_var(&ring, "transform", &format!("translate3d({nx:.1}px,{ny:.1}px,0)"));
                    let moving = (tx - nx).abs() > 0.2 || (ty - ny).abs() > 0.2;
                    running.set(moving);
                    moving
                });
            }
        });
    }
    {
        let (dot, ring) = (dot.clone(), ring.clone());
        on(&document(), "mouseleave", move |_: web_sys::Event| {
            let _ = dot.class_list().remove_1("is-visible");
            let _ = ring.class_list().remove_1("is-visible");
        });
    }
    {
        let ring = ring.clone();
        on_passive(&document(), "pointerdown", move |_: web_sys::Event| {
            let _ = ring.class_list().add_1("is-down");
        });
    }
    on_passive(&document(), "pointerup", move |_: web_sys::Event| {
        let _ = ring.class_list().remove_1("is-down");
    });
}

type Slot = Rc<RefCell<Option<HtmlElement>>>;

fn closest(target: &Element, selector: &str) -> Option<HtmlElement> {
    target.closest(selector).ok().flatten().and_then(|el| el.dyn_into::<HtmlElement>().ok())
}

/// Remembers the hovered element; resets the previous one when it changes.
fn swap(slot: &Slot, next: Option<&HtmlElement>, reset: &[&str]) {
    let mut current = slot.borrow_mut();
    let same = matches!((current.as_ref(), next), (Some(a), Some(b)) if a == b);
    if !same {
        if let Some(previous) = current.take() {
            for prop in reset {
                let _ = previous.style().remove_property(prop);
            }
        }
        *current = next.cloned();
    }
}

/// One delegated listener: `.tilt` cards tilt in 3D (`--rx`, `--ry`), `.spotlight` glows
/// follow the pointer (`--mx`, `--my`), `.magnetic` buttons lean towards it (`--tx`,
/// `--ty`), and the background drifts (parallax).
fn pointer_effects() {
    let tilt: Slot = Rc::default();
    let magnet: Slot = Rc::default();
    let background = all(".bg");
    on_passive(&document(), "pointermove", move |e: PointerEvent| {
        if e.pointer_type() != "mouse" {
            return;
        }
        let (cx, cy) = (f64::from(e.client_x()), f64::from(e.client_y()));
        let width = window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1.0);
        for bg in &background {
            set_var(bg, "--mx", &format!("{:.3}", cx / width * 2.0 - 1.0));
            set_var(bg, "--my", &format!("{:.3}", cy / viewport_height() * 2.0 - 1.0));
        }
        let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        if let Some(el) = closest(&target, ".spotlight") {
            let r = el.get_bounding_client_rect();
            set_var(&el, "--mx", &format!("{:.1}px", cx - r.left()));
            set_var(&el, "--my", &format!("{:.1}px", cy - r.top()));
        }
        let tilt_el = closest(&target, ".tilt");
        if let Some(el) = &tilt_el {
            let r = el.get_bounding_client_rect();
            let x = ((cx - r.left()) / r.width().max(1.0)).clamp(0.0, 1.0);
            let y = ((cy - r.top()) / r.height().max(1.0)).clamp(0.0, 1.0);
            set_var(el, "--rx", &format!("{:.2}deg", (0.5 - y) * 9.0));
            set_var(el, "--ry", &format!("{:.2}deg", (x - 0.5) * 11.0));
        }
        swap(&tilt, tilt_el.as_ref(), &["--rx", "--ry"]);
        let magnet_el = closest(&target, ".magnetic");
        if let Some(el) = &magnet_el {
            let r = el.get_bounding_client_rect();
            let dx = cx - (r.left() + r.width() / 2.0);
            let dy = cy - (r.top() + r.height() / 2.0);
            set_var(el, "--tx", &format!("{:.1}px", dx * 0.22));
            set_var(el, "--ty", &format!("{:.1}px", dy * 0.3));
        }
        swap(&magnet, magnet_el.as_ref(), &["--tx", "--ty"]);
    });
}
