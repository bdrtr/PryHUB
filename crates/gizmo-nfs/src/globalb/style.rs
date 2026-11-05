//! The style events: what the game pays points for, and how — `GLOBALB.BUN` chunk `0x00034A07`.
//!
//! `SPEED2.EXE` loads this chunk by id (`0x56C4C0`: the payload rounded up to 16 bytes) and keeps a
//! pointer to it; at the start of every event it allocates one live event per record whose mode
//! bits match the event being run (`0x56EBE0`). Thirty-nine records of `0x30` bytes. Each record
//! names itself through `LANGUAGES/*.bin` — the first word is a label hash, and `English.bin`
//! resolves all but four: `DRIFT`, `POWER SLIDE`, `HANG TIME`, `DRAFTING`, `360`, `J-TURN`, …
//! ([`crate::language::labels`]).
//!
//! # What the game does with a record
//!
//! Read from the scorer (`0x56C740`, `0x56E920`, `0x56EAB0`, `0x56D500`):
//!
//! - A **continuous** event ([`StyleEvent::continuous`]: drift, power slide, hang time, drafting,
//!   reverse) is fed an *input* every frame it is happening, and accrues
//!   `rate · input · multiplier · dt` points, capped at [`StyleEvent::cap`] when that is positive.
//!   It has to run for [`StyleEvent::settle_s`] seconds and pass [`StyleEvent::threshold`] points
//!   before it counts at all; once it stops being fed for [`StyleEvent::wait_s`] seconds, what it
//!   has accrued is banked. Stopped before it counted, it pays nothing.
//! - An **instant** event (a 360, a J-turn, a clean section) pays `rate · input · multiplier` the
//!   moment it fires, if that clears the threshold (or falls under it, for the negative-rate
//!   penalties: `WALL IMPACT`, `TRAFFIC HIT`).
//! - The **multiplier** starts at 1 and grows by [`StyleEvent::multiplier_step`] each time the event
//!   is banked and fired again, up to [`StyleEvent::max_multiplier`]. The drift scorer sets it
//!   itself, from its own five-level combo, which is why the drift record's step is 0.
//! - [`StyleEvent::limit`] is how many times one race pays it (`-1`: no limit).
//!
//! The player's banked total refills the nitrous (`0x6016A0`): `Δpoints / 15 · 0.01` of the tank,
//! so 1 500 points are one tank.

use crate::chunk::ChunkNode;
use crate::error::NfsResult;

/// The chunk that holds the definitions.
pub const CHUNK: u32 = 0x0003_4A07;
/// One record's size.
pub const RECORD: usize = 0x30;

/// One style event as the game defines it. Field names are what the scorer does with each lane;
/// see the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleEvent {
    /// The label's hash in `LANGUAGES/*.bin`.
    pub label: u32,
    /// The record's own index, `0..39`, stored at `+4`.
    pub index: u8,
    /// `+5`: the index again, except on the seven drift tiers (`Good Drift!` … `Drift King!`),
    /// which carry `0` — the drift they are tiers of.
    pub parent: u8,
    /// `+6`: accrues over time rather than paying at once.
    pub continuous: bool,
    /// `+7`: the kinds of event it runs in, as bits. Bit `0x04` is the drift event (every drift
    /// record has it and nothing else does); the others are not named here.
    pub modes: u8,
    /// `+8`: how long a continuous event must run before it counts, seconds. On the drift tiers it
    /// is a points mark instead (1 000 … 100 000).
    pub settle_s: f32,
    /// `+0xC`: how long a continuous event may go unfed before it is banked, seconds.
    pub wait_s: f32,
    /// `+0x10`: the points it must pass to count (below, for a negative-rate penalty).
    pub threshold: f32,
    /// `+0x14`: the most one occurrence pays; `0` is no cap.
    pub cap: f32,
    /// `+0x18`: the highest the multiplier goes.
    pub max_multiplier: f32,
    /// `+0x1C`: how much the multiplier grows on each repeat.
    pub multiplier_step: f32,
    /// `+0x20`, `+0x24`: `0.5` on the drift record, `1.0` everywhere else; not read by the scorer.
    pub unknown: [f32; 2],
    /// `+0x28`: points per unit of input (per second, for a continuous event).
    pub rate: f32,
    /// `+0x2C`: how many times one race pays it, `-1` without limit.
    pub limit: i8,
}

/// Every style event in a decompressed `GLOBALB.BUN`, in record order. Empty if the chunk is absent.
///
/// # Errors
///
/// When the chunk tree cannot be walked.
pub fn style_events(globalb: &[u8]) -> NfsResult<Vec<StyleEvent>> {
    let roots = ChunkNode::parse(globalb)?;
    let Some(node) = roots
        .iter()
        .find(|r| r.header.id == CHUNK)
        .or_else(|| roots.iter().find_map(|r| r.find(CHUNK)))
    else {
        return Ok(Vec::new());
    };
    // The game rounds the payload's address up to 16; the file is laid out so that the same
    // rounding of the file offset lands on the same byte.
    let start = (node.data_offset + 0xF) & !0xF;
    let end = node.data_offset + node.header.size as usize;
    let mut out = Vec::new();
    let mut at = start;
    while at + RECORD <= end.min(globalb.len()) {
        out.push(read(&globalb[at..at + RECORD]));
        at += RECORD;
    }
    Ok(out)
}

fn read(r: &[u8]) -> StyleEvent {
    let f = |o: usize| f32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
    StyleEvent {
        label: u32::from_le_bytes([r[0], r[1], r[2], r[3]]),
        index: r[4],
        parent: r[5],
        continuous: r[6] != 0,
        modes: r[7],
        settle_s: f(0x08),
        wait_s: f(0x0C),
        threshold: f(0x10),
        cap: f(0x14),
        max_multiplier: f(0x18),
        multiplier_step: f(0x1C),
        unknown: [f(0x20), f(0x24)],
        rate: f(0x28),
        limit: r[0x2C] as i8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_reads_lane_by_lane() {
        let mut r = vec![0u8; RECORD];
        r[0..4].copy_from_slice(&0xA46A_AEA8u32.to_le_bytes());
        r[4] = 0;
        r[6] = 1;
        r[7] = 4;
        r[0x0C..0x10].copy_from_slice(&0.3f32.to_le_bytes());
        r[0x10..0x14].copy_from_slice(&50.0f32.to_le_bytes());
        r[0x28..0x2C].copy_from_slice(&120.0f32.to_le_bytes());
        r[0x2C] = 0xFF;
        let e = read(&r);
        assert_eq!(e.label, 0xA46A_AEA8);
        assert!(e.continuous && e.modes == 4);
        assert_eq!((e.wait_s, e.threshold, e.rate, e.limit), (0.3, 50.0, 120.0, -1));
    }
}
