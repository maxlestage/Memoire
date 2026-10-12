//! Static files (`site/static`) and the browser bundle (`site/pkg`), read once at startup
//! and served from memory. Text files are compressed once (brotli and gzip), the first time
//! they are asked for, or right after startup by [`Assets::warm`].

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

pub struct Asset {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    /// Short content hash, used in `?v=` so that files can be cached forever.
    pub version: String,
    brotli: OnceLock<Option<Vec<u8>>>,
    gzip: OnceLock<Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Assets {
    files: BTreeMap<String, Asset>,
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// FNV-1a, enough to tell versions apart.
pub fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

pub fn short_hash(bytes: &[u8]) -> String {
    format!("{:016x}", fnv(0xcbf2_9ce4_8422_2325, bytes))[..10].to_string()
}

impl Asset {
    pub fn new(bytes: Vec<u8>, content_type: &'static str) -> Asset {
        Asset { version: short_hash(&bytes), bytes, content_type, brotli: OnceLock::new(), gzip: OnceLock::new() }
    }

    /// Text, styles, scripts, WebAssembly, SVG: worth compressing.
    fn compressible(&self) -> bool {
        self.bytes.len() >= 512
            && (self.content_type.starts_with("text/")
                || self.content_type.starts_with("application/")
                || self.content_type == "image/svg+xml")
    }

    /// The file compressed with brotli, if that makes it smaller.
    pub fn brotli(&self) -> Option<&[u8]> {
        self.brotli
            .get_or_init(|| {
                if !self.compressible() {
                    return None;
                }
                let params = brotli::enc::BrotliEncoderParams { quality: 11, lgwin: 22, ..Default::default() };
                let mut out = Vec::new();
                brotli::enc::BrotliCompress(&mut &self.bytes[..], &mut out, &params).ok()?;
                (out.len() < self.bytes.len()).then_some(out)
            })
            .as_deref()
    }

    /// The file compressed with gzip, if that makes it smaller.
    pub fn gzip(&self) -> Option<&[u8]> {
        self.gzip
            .get_or_init(|| {
                if !self.compressible() {
                    return None;
                }
                let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
                encoder.write_all(&self.bytes).ok()?;
                let out = encoder.finish().ok()?;
                (out.len() < self.bytes.len()).then_some(out)
            })
            .as_deref()
    }
}

impl Assets {
    /// Loads every file of `dir` (recursively) under the URL prefix `prefix`.
    pub fn load(&mut self, dir: &Path, prefix: &str) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                self.load(&path, &format!("{prefix}/{name}"));
            } else if let Ok(bytes) = std::fs::read(&path) {
                self.files.insert(format!("{prefix}/{name}"), Asset::new(bytes, content_type(&name)));
            }
        }
    }

    pub fn get(&self, url: &str) -> Option<&Asset> {
        self.files.get(url)
    }

    /// The link to `url`, behind the base path: `url?v=<hash>` for files that exist.
    pub fn url(&self, url: &str) -> String {
        let link = crate::routes::link(url);
        match self.files.get(url) {
            Some(asset) => format!("{link}?v={}", asset.version),
            None => link,
        }
    }

    /// Every file, with its URL, sorted by URL (static export, service worker version).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Asset)> {
        self.files.iter().map(|(url, asset)| (url.as_str(), asset))
    }

    /// Compresses every file now (the server calls it in the background at startup).
    pub fn warm(&self) {
        for asset in self.files.values() {
            asset.brotli();
            asset.gzip();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_text_but_not_images() {
        let css = Asset::new("body { color: red; }\n".repeat(100).into_bytes(), content_type("app.css"));
        assert!(css.brotli().unwrap().len() < css.bytes.len());
        assert!(css.gzip().unwrap().len() < css.bytes.len());
        let png = Asset::new(vec![7; 4096], content_type("icon.png"));
        assert!(png.brotli().is_none() && png.gzip().is_none());
        assert_eq!(content_type("og-image.jpg"), "image/jpeg");
        assert_eq!(content_type("manifest.webmanifest"), "application/manifest+json");
    }
}
