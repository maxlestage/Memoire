//! The few browser services the islands need (clock, clipboard, timers), with the same API
//! on the server, where event handlers and mount callbacks never run: there they do
//! nothing (the clock still works, for the playground's first render).

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    fn window() -> web_sys::Window {
        web_sys::window().expect_throw("no window")
    }

    pub fn now_ms() -> f64 {
        window().performance().map_or(0.0, |p| p.now())
    }

    pub fn copy_text(text: &str) {
        let promise = window().navigator().clipboard().write_text(text);
        // Swallow rejections (insecure context, denied permission).
        let ignore = Closure::<dyn FnMut(JsValue)>::new(|_| {});
        let _ = promise.catch(&ignore);
        ignore.forget();
    }

    pub fn after(ms: i32, f: impl FnOnce() + 'static) {
        let f = Closure::once_into_js(f);
        let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(f.unchecked_ref(), ms);
    }

    /// A `setInterval`, cleared when dropped.
    pub struct Interval {
        id: i32,
        _closure: Closure<dyn FnMut()>,
    }

    impl Drop for Interval {
        fn drop(&mut self) {
            window().clear_interval_with_handle(self.id);
        }
    }

    pub fn every(ms: i32, f: impl FnMut() + 'static) -> Interval {
        let closure = Closure::<dyn FnMut()>::new(f);
        let id = window()
            .set_interval_with_callback_and_timeout_and_arguments_0(closure.as_ref().unchecked_ref(), ms)
            .unwrap_or_default();
        Interval { id, _closure: closure }
    }

    pub fn reduced_motion() -> bool {
        window().match_media("(prefers-reduced-motion: reduce)").ok().flatten().is_some_and(|m| m.matches())
    }

    /// A slot holding at most one running interval (replacing it stops the previous one).
    pub type Timer = Rc<RefCell<Option<Interval>>>;
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::cell::RefCell;
    use std::rc::Rc;

    pub fn now_ms() -> f64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }

    pub fn copy_text(_text: &str) {}

    pub fn after(_ms: i32, _f: impl FnOnce() + 'static) {}

    pub struct Interval;

    pub fn every(_ms: i32, _f: impl FnMut() + 'static) -> Interval {
        Interval
    }

    pub fn reduced_motion() -> bool {
        false
    }

    pub type Timer = Rc<RefCell<Option<Interval>>>;
}

pub use imp::*;
