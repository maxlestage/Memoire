//! `memoire-site-server precompress <dir>`: writes `.br` (quality 11) and `.gz`
//! (level 9) siblings for every compressible file, so the server can send them
//! as-is instead of compressing on each request. Used by the Dockerfile; no
//! extra system packages needed.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

const EXTENSIONS: &[&str] = &["wasm", "js", "mjs", "css", "html", "svg", "txt", "json", "xml", "ico", "map"];
/// Not worth compressing below this size.
const MIN_SIZE: u64 = 512;

pub fn run(dir: &Path) -> io::Result<()> {
    let mut stats = (0usize, 0u64, 0u64);
    walk(dir, &mut stats)?;
    println!(
        "precompressed {} files: {} KiB -> {} KiB (brotli)",
        stats.0,
        stats.1 / 1024,
        stats.2 / 1024
    );
    Ok(())
}

fn walk(dir: &Path, stats: &mut (usize, u64, u64)) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, stats)?;
            continue;
        }
        let compressible = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
        if !compressible || fs::metadata(&path)?.len() < MIN_SIZE {
            continue;
        }
        let data = fs::read(&path)?;

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&data)?;
        let gz = gz.finish()?;

        let params = brotli::enc::BrotliEncoderParams {
            quality: 11,
            lgwin: 22,
            ..Default::default()
        };
        let mut br = Vec::new();
        brotli::enc::BrotliCompress(&mut &data[..], &mut br, &params)?;

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        fs::write(path.with_file_name(format!("{name}.gz")), &gz)?;
        fs::write(path.with_file_name(format!("{name}.br")), &br)?;
        stats.0 += 1;
        stats.1 += data.len() as u64;
        stats.2 += br.len() as u64;
    }
    Ok(())
}
