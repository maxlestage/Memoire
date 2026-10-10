//! Light / dark theme, driven by `data-theme` on `<html>` and CSS variables.

use yew::prelude::*;

use crate::dom;

const STORAGE_KEY: &str = "memoire-theme";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    pub fn toggled(self) -> Theme {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }

    /// Saved choice first, then `prefers-color-scheme`, defaulting to dark.
    pub fn detect() -> Theme {
        match dom::storage_get(STORAGE_KEY).as_deref() {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            _ if dom::media_matches("(prefers-color-scheme: light)") => Theme::Light,
            _ => Theme::Dark,
        }
    }

    /// Apply to the document. `persist` is false for the initial detection so a
    /// first-time visitor keeps following their OS setting.
    pub fn apply(self, persist: bool) {
        if persist {
            dom::storage_set(STORAGE_KEY, self.as_str());
        }
        if let Some(root) = dom::document().document_element() {
            let _ = root.set_attribute("data-theme", self.as_str());
        }
        dom::set_meta(
            "theme-color",
            match self {
                Theme::Dark => "#07070d",
                Theme::Light => "#f7f7fb",
            },
        );
    }
}

#[derive(Clone, PartialEq)]
pub struct ThemeContext {
    pub theme: Theme,
    pub toggle: Callback<()>,
}

#[hook]
pub fn use_theme() -> ThemeContext {
    use_context::<ThemeContext>().expect("ThemeContext provider missing")
}
