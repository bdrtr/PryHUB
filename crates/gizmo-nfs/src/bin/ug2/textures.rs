//! `ug2 textures` — the car's texture table, and which material run uses which image.

use crate::paths::{Car, Result};
use std::path::Path;

pub fn run(car: &Path, filter: Option<&str>) -> Result<()> {
    let car = Car::resolve(car)?;
    let parts = car.parts()?;
    let Some(tpk) = car.textures() else {
        return Err(format!("{}: no readable TEXTURES.BIN", car.dir.display()));
    };

    // What the file declares and what came back as pixels are two numbers, and saying only the
    // second reports a short pack as a whole one — which is exactly what a tool for asking "what is
    // in this file" must not do. The app draws the same subtraction beside its contact sheet.
    let undecoded = tpk.entries.len().saturating_sub(tpk.textures.len());
    if undecoded > 0 {
        outln!("== {} textures ==  ({undecoded} declared but not decoded)", tpk.textures.len());
    } else {
        outln!("== {} textures ==", tpk.textures.len());
    }
    let mut texs: Vec<_> = tpk.textures.values().collect();
    // By name, then by hash: names are truncated to a fixed field, so two textures routinely share
    // one, and a tie broken by `HashMap` order made two runs of the same command differ.
    texs.sort_by(|a, b| a.name.cmp(&b.name).then(a.hash.0.cmp(&b.hash.0)));
    for t in &texs {
        // opaque% and mean luminance say whether a map is an alpha overlay (mostly transparent)
        // or a full-coverage image — the difference that decides whether it can be composited
        // over the paint as a detail layer.
        let opaque = t.opaque_permille() / 10;
        let texels = (t.rgba.len() / 4).max(1);
        let lum: usize =
            t.rgba.chunks_exact(4).map(|px| (px[0] as usize + px[1] as usize + px[2] as usize) / 3).sum();
        outln!(
            "  {:#010x}  {:>4}x{:<4}  {:?}  opaque={opaque:>3}%  lum={:>3}  {}",
            t.hash.0,
            t.width,
            t.height,
            t.source_format,
            lum / texels,
            t.name
        );
    }

    outln!("\n== material runs (shader | texture hash → resolved texture) ==");
    for p in parts.iter().filter(|p| filter.is_none_or(|f| p.name.contains(f))) {
        if p.materials.is_empty() {
            continue;
        }
        outln!("• {} ({} runs)", p.name, p.materials.len());
        for m in &p.materials {
            let resolved = tpk.texture(m.hash).map(|t| t.name.as_str()).unwrap_or("—");
            outln!(
                "    shader={:#010x}  tex={:#010x} → {:<28}  tris={}",
                m.shader.0,
                m.hash.0,
                resolved,
                m.index_count / 3
            );
        }
    }
    Ok(())
}
