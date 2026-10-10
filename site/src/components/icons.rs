//! Inline SVG icons (stroke = currentColor) rendered as raw HTML.

use yew::prelude::*;

macro_rules! svg {
    ($body:literal) => {
        concat!(
            r#"<svg class="icon" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">"#,
            $body,
            "</svg>"
        )
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Github,
    ArrowRight,
    ArrowLeft,
    ArrowUp,
    Copy,
    Check,
    Sun,
    Moon,
    Search,
    External,
    Book,
    Edit,
    Clock,
    Menu,
    Close,
    ChevronRight,
    Gear,
    Code,
    Shield,
    Zap,
    Database,
    Cpu,
    Activity,
    Layers,
    Globe,
    Play,
    Terminal,
    Crate,
}

impl Icon {
    pub fn svg(self) -> &'static str {
        match self {
            Icon::Github => r#"<svg class="icon" viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.65.5.5 5.65.5 12a11.5 11.5 0 0 0 7.86 10.92c.58.1.79-.25.79-.56v-2c-3.2.7-3.87-1.37-3.87-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.71 1.26 3.37.96.1-.75.4-1.26.73-1.55-2.56-.29-5.25-1.28-5.25-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.7 5.38-5.26 5.67.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5z"/></svg>"#,
            Icon::ArrowRight => svg!(r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#),
            Icon::ArrowLeft => svg!(r#"<path d="M19 12H5M11 18l-6-6 6-6"/>"#),
            Icon::ArrowUp => svg!(r#"<path d="M12 19V5M6 11l6-6 6 6"/>"#),
            Icon::Copy => svg!(r#"<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>"#),
            Icon::Check => svg!(r#"<path d="M20 6 9 17l-5-5"/>"#),
            Icon::Sun => svg!(r#"<circle cx="12" cy="12" r="4.5"/><path d="M12 1.5v2.5M12 20v2.5M4.6 4.6l1.8 1.8M17.6 17.6l1.8 1.8M1.5 12H4M20 12h2.5M4.6 19.4l1.8-1.8M17.6 6.4l1.8-1.8"/>"#),
            Icon::Moon => svg!(r#"<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>"#),
            Icon::Search => svg!(r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#),
            Icon::External => svg!(r#"<path d="M14 4h6v6M20 4 10 14M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>"#),
            Icon::Book => svg!(r#"<path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5v14zM4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5"/>"#),
            Icon::Edit => svg!(r#"<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>"#),
            Icon::Clock => svg!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"#),
            Icon::Menu => svg!(r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#),
            Icon::Close => svg!(r#"<path d="M6 6l12 12M18 6 6 18"/>"#),
            Icon::ChevronRight => svg!(r#"<path d="m9 6 6 6-6 6"/>"#),
            Icon::Gear => svg!(r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>"#),
            Icon::Code => svg!(r#"<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>"#),
            Icon::Shield => svg!(r#"<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/>"#),
            Icon::Zap => svg!(r#"<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>"#),
            Icon::Database => svg!(r#"<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.7-4 3-9 3s-9-1.3-9-3M3 5v14c0 1.7 4 3 9 3s9-1.3 9-3V5"/>"#),
            Icon::Cpu => svg!(r#"<rect x="5" y="5" width="14" height="14" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4"/>"#),
            Icon::Activity => svg!(r#"<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>"#),
            Icon::Layers => svg!(r#"<path d="m12 2 10 5-10 5L2 7l10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/>"#),
            Icon::Globe => svg!(r#"<circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>"#),
            Icon::Play => svg!(r#"<path d="m6 4 14 8-14 8V4z"/>"#),
            Icon::Terminal => svg!(r#"<path d="m4 17 6-6-6-6M12 19h8"/>"#),
            Icon::Crate => svg!(r#"<path d="M21 8 12 3 3 8v8l9 5 9-5V8z"/><path d="m3 8 9 5 9-5M12 13v8"/>"#),
        }
    }

    pub fn html(self) -> Html {
        Html::from_html_unchecked(AttrValue::Static(self.svg()))
    }
}

/// Icons used for the 8 feature cards, in the same order as `Texts::features`.
pub const FEATURE_ICONS: [Icon; 8] = [
    Icon::Gear,
    Icon::Code,
    Icon::Shield,
    Icon::Zap,
    Icon::Database,
    Icon::Cpu,
    Icon::Activity,
    Icon::Layers,
];
