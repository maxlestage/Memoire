//! The Memoire ORM website, written with active.
//!
//! The library is shared by both sides: texts, routes, documentation titles, search and
//! the islands (the playground runs the real ORM). Compiled for the server it also holds
//! the Vitesse server and the static export (`server`); compiled to WebAssembly it is the
//! browser bundle (`client`): it hydrates the islands and runs the page motion.

pub mod data;
pub mod docs;
pub mod highlight;
pub mod i18n;
pub mod icons;
pub mod islands;
pub mod playground;
pub mod routes;
pub mod search;
pub mod web;

/// The theme color of the dark theme (the default): status bar, splash screen.
pub const THEME_DARK: &str = "#07070d";
/// The theme color of the light theme.
pub const THEME_LIGHT: &str = "#f6f6fb";

#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[cfg(target_arch = "wasm32")]
mod client;
