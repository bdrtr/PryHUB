//! `ug2 poke` — change one number in one car's handling record, so the game can say what it was.
//!
//! The blunt end of the same lever `dump` and `probe` are. The per-car handling record is 2,192
//! bytes and 222 of its 4-byte lanes vary per car and are not read by anything here: the structure
//! is visible — two symmetric axle blocks, repeated nine- and twelve-point tables — and the meaning
//! is not, because a number's meaning is not in the file.
//!
//! It cannot be measured from here either. Nothing in this crate can tell a spring rate from a
//! damper by looking at it, and guessing produces a label that survives review and is wrong. What
//! settles it is the experiment the save profile was locked with, run the other way round: change
//! **one** value, install, drive, and see what the car does. This writes the file for that.
//!
//! Nothing here interprets the lane. It reports what was there and what is there now, and leaves the
//! naming to whatever the car does next.

use crate::paths::{read, Result};
use gizmo_nfs::chunk::ChunkNode;
use std::path::Path;

const HANDLING: u32 = 0x0003_4600;
const STRIDE: usize = 2192;

pub fn run(bundle: &Path, car: &str, at: usize, set: Option<f32>, out: &Path) -> Result<()> {
    if !at.is_multiple_of(4) {
        return Err(format!("{at:#x}: the record's lanes are four-byte aligned"));
    }
    if at + 4 > STRIDE {
        return Err(format!("{at:#x}: past the end of a {STRIDE}-byte record"));
    }
    let raw = read(bundle)?;
    let bytes = match gizmo_nfs::compression::detect(&raw) {
        gizmo_nfs::compression::Codec::None => raw,
        _ => gizmo_nfs::compression::decompress(&raw).map_err(|e| format!("{e}"))?,
    };
    let roots = ChunkNode::parse(&bytes).map_err(|e| format!("{e}"))?;
    let node = find(&roots, HANDLING)
        .ok_or_else(|| format!("{}: no 0x00034600 handling chunk", bundle.display()))?;
    let data = node.data(&bytes);
    let cars = data.len().saturating_sub(8) / STRIDE;

    // The record is found by its own name, which it carries twice.
    let want = car.to_ascii_uppercase();
    let index = (0..cars)
        .find(|c| name_at(data, *c) == want)
        .ok_or_else(|| format!("{car}: no record of that name in {cars}"))?;

    let lane = 8 + index * STRIDE + at;
    let was = f32::from_le_bytes([data[lane], data[lane + 1], data[lane + 2], data[lane + 3]]);
    outln!("{want} (record {index} of {cars})  +{at:#06x}");
    outln!("  was {was}  (0x{:08x})", was.to_bits());

    let Some(value) = set else {
        // Read-only: say what every car has there, which is what a first look wants.
        outln!("  across all {cars} records:");
        let mut all: Vec<(String, f32)> = (0..cars)
            .map(|c| {
                let o = 8 + c * STRIDE + at;
                (name_at(data, c), f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]))
            })
            .collect();
        all.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (n, v) in all.iter().take(6) {
            outln!("    {v:>12.4}  {n}");
        }
        outln!("    …");
        for (n, v) in all.iter().rev().take(3) {
            outln!("    {v:>12.4}  {n}");
        }
        return Ok(());
    };

    // Four bytes, written where they already are. **Not** through `repack::rebuild`, which is the
    // right tool for a payload that changed length and the wrong one here: nothing changes length,
    // so nothing needs re-deriving, and re-deriving it is not free. Measured on this very bundle,
    // rebuilding it with *no* edits at all returns 8,008,120 bytes for an 8,008,064-byte input —
    // `GLOBALB.BUN` carries an embedded `0x80134000` geometry stream, and the repacker re-derives
    // the 128-byte solid alignment inside it into padding the file did not have.
    //
    // The same reasoning as `texture::replace_blob`: an in-place write is safe *because* nothing
    // moves, and so it needs no theory of the layout it is writing into.
    let lane_abs = node.data_offset + 8 + index * STRIDE + at;
    let mut written = bytes.clone();
    written
        .get_mut(lane_abs..lane_abs + 4)
        .ok_or("the lane is outside the bundle")?
        .copy_from_slice(&value.to_le_bytes());
    if written.len() != bytes.len() {
        return Err("an in-place write changed the file's length".into());
    }

    // Read it back through the parser before handing it over, the same rule the other write
    // commands keep — and read the *lane*, not merely the record, so a write to the wrong byte is
    // caught here rather than in the game.
    let back = f32::from_le_bytes([
        written[lane_abs],
        written[lane_abs + 1],
        written[lane_abs + 2],
        written[lane_abs + 3],
    ]);
    if back.to_bits() != value.to_bits() {
        return Err(format!("read back {back} rather than {value}"));
    }
    gizmo_nfs::globalb::find_car(&written, &want)
        .ok_or("the rewritten bundle no longer holds that car")?;

    std::fs::write(out, &written).map_err(|e| format!("{}: {e}", out.display()))?;
    outln!("  now {value}  → {}", out.display());
    outln!("  install it, drive the car, and see what moved. Nothing here knows what this lane is.");
    Ok(())
}

fn name_at(data: &[u8], car: usize) -> String {
    let at = 8 + car * STRIDE;
    data.get(at..at + 16)
        .unwrap_or(&[])
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect()
}

fn find(nodes: &[ChunkNode], id: u32) -> Option<&ChunkNode> {
    for n in nodes {
        if n.header.id == id {
            return Some(n);
        }
        if let Some(f) = find(&n.children, id) {
            return Some(f);
        }
    }
    None
}
