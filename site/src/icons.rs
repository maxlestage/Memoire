//! Inline SVG icons (stroke = currentColor), built as active elements so that they render
//! on the server and can be created by islands in the browser.

use active::Element;
use active::html::svg;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// The SVG children (flat list of self-closing elements).
    fn body(self) -> &'static str {
        match self {
            Icon::Github => {
                r#"<path fill="currentColor" stroke="none" d="M12 .5C5.65.5.5 5.65.5 12a11.5 11.5 0 0 0 7.86 10.92c.58.1.79-.25.79-.56v-2c-3.2.7-3.87-1.37-3.87-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.71 1.26 3.37.96.1-.75.4-1.26.73-1.55-2.56-.29-5.25-1.28-5.25-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.7 5.38-5.26 5.67.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5z"/>"#
            }
            Icon::ArrowRight => r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#,
            Icon::ArrowLeft => r#"<path d="M19 12H5M11 18l-6-6 6-6"/>"#,
            Icon::ArrowUp => r#"<path d="M12 19V5M6 11l6-6 6 6"/>"#,
            Icon::Copy => {
                r#"<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>"#
            }
            Icon::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
            Icon::Sun => {
                r#"<circle cx="12" cy="12" r="4.5"/><path d="M12 1.5v2.5M12 20v2.5M4.6 4.6l1.8 1.8M17.6 17.6l1.8 1.8M1.5 12H4M20 12h2.5M4.6 19.4l1.8-1.8M17.6 6.4l1.8-1.8"/>"#
            }
            Icon::Moon => r#"<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>"#,
            Icon::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
            Icon::External => {
                r#"<path d="M14 4h6v6M20 4 10 14M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>"#
            }
            Icon::Book => {
                r#"<path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5v14zM4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5"/>"#
            }
            Icon::Edit => r#"<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>"#,
            Icon::Clock => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"#,
            Icon::Menu => r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#,
            Icon::Close => r#"<path d="M6 6l12 12M18 6 6 18"/>"#,
            Icon::ChevronRight => r#"<path d="m9 6 6 6-6 6"/>"#,
            Icon::Gear => {
                r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>"#
            }
            Icon::Code => r#"<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>"#,
            Icon::Shield => r#"<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/>"#,
            Icon::Zap => r#"<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>"#,
            Icon::Database => {
                r#"<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.7-4 3-9 3s-9-1.3-9-3M3 5v14c0 1.7 4 3 9 3s9-1.3 9-3V5"/>"#
            }
            Icon::Cpu => {
                r#"<rect x="5" y="5" width="14" height="14" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4"/>"#
            }
            Icon::Activity => r#"<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>"#,
            Icon::Layers => r#"<path d="m12 2 10 5-10 5L2 7l10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/>"#,
            Icon::Globe => {
                r#"<circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>"#
            }
            Icon::Play => r#"<path d="m6 4 14 8-14 8V4z"/>"#,
            Icon::Terminal => r#"<path d="m4 17 6-6-6-6M12 19h8"/>"#,
            Icon::Crate => r#"<path d="M21 8 12 3 3 8v8l9 5 9-5V8z"/><path d="m3 8 9 5 9-5M12 13v8"/>"#,
        }
    }
}

/// Icons used for the 8 feature cards, in the same order as `Texts::features`.
pub const FEATURE_ICONS: [Icon; 8] =
    [Icon::Gear, Icon::Code, Icon::Shield, Icon::Zap, Icon::Database, Icon::Cpu, Icon::Activity, Icon::Layers];

/// `<svg class="icon" …>` with the icon's shapes.
pub fn icon(icon: Icon) -> Element {
    let filled = icon == Icon::Github;
    let root = svg()
        .class("icon")
        .attr("viewBox", "0 0 24 24")
        .attr("width", "20")
        .attr("height", "20")
        .attr("fill", if filled { "currentColor" } else { "none" });
    let root = if filled {
        root
    } else {
        root.attr("stroke", "currentColor")
            .attr("stroke-width", "2")
            .attr("stroke-linecap", "round")
            .attr("stroke-linejoin", "round")
    };
    shapes(icon.body()).into_iter().fold(root.attr("aria-hidden", "true"), |svg, shape| svg.child(shape))
}

/// Parses a flat list of self-closing SVG elements (`<path d="…"/><circle cx="1"/>`).
pub fn shapes(markup: &str) -> Vec<Element> {
    let mut out = Vec::new();
    let mut rest = markup;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find("/>") else { break };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 2..];
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let mut element = active::el(static_tag(&tag[..name_end]));
        let mut attrs = &tag[name_end..];
        while let Some(eq) = attrs.find("=\"") {
            let name = attrs[..eq].trim();
            let Some(end) = attrs[eq + 2..].find('"') else { break };
            element = element.attr(name.to_string(), attrs[eq + 2..eq + 2 + end].to_string());
            attrs = &attrs[eq + 3 + end..];
        }
        out.push(element);
    }
    out
}

fn static_tag(name: &str) -> &'static str {
    match name {
        "circle" => "circle",
        "rect" => "rect",
        "ellipse" => "ellipse",
        "line" => "line",
        "polyline" => "polyline",
        "polygon" => "polygon",
        _ => "path",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_render_as_svg() {
        let html = active::Node::from(icon(Icon::Database)).render();
        assert!(html.starts_with(r#"<svg class="icon" viewBox="0 0 24 24""#), "{html}");
        assert!(html.contains(r#"<ellipse cx="12" cy="5" rx="9" ry="3"></ellipse>"#), "{html}");
        let github = active::Node::from(icon(Icon::Github)).render();
        assert!(github.contains(r#"fill="currentColor""#));
    }
}
