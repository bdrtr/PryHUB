# PryHUB

A **Need for Speed: Underground 2** (2004) asset toolkit for Linux: open the game's own files,
browse them, be told what looks wrong, work out what the undecoded parts mean, and export what you
find. Two crates, one purpose:

- **[`crates/gizmo-nfs`](crates/gizmo-nfs)** — the parser. Pure, engine-agnostic, `#![forbid(unsafe_code)]`,
  panic-free on untrusted bytes, no graphics dependencies. Reads NFSU2's containers and returns
  plain CPU data. Also ships the **`ug2`** command-line tool.
- **[`crates/pryhub`](crates/pryhub)** — the app. A native [egui](https://github.com/emilk/egui)
  interface over one open file: chunk tree · hex · 3D · textures · inspector · log, plus validation,
  discovery, compare and dictionary screens.

Nothing here contains game data. You need your own copy of the game; files are read at runtime from
a path you give.

## Why it exists

The reference tool for these files (NFS-CarToolkit) is Windows-only and closed. Rather than imitate
it, PryHUB leans on what it can do differently:

| CarToolkit | PryHUB |
|---|---|
| Windows / .NET | Linux-native (cross-platform toolkit) |
| Closed source | Open source |
| Fixed parser | **Discovery mode** — read an undecoded chunk with a layout you type |
| Silent failure | **Validation** — every check says what it examined, so "no findings" is never confused with "nobody looked" |
| Mostly OBJ | glTF (`.glb`, self-contained) + OBJ/MTL + PNG |

## Build & run

```bash
# The parser and its tests — fast, no game install needed (tests are synthetic):
cargo test -p gizmo-nfs

# The app:
export NFSU2_ROOT="/path/to/Need for Speed Underground 2"
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/240SX/GEOMETRY.BIN"
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/240SX/TEXTURES.BIN" --tab texture
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/RX7/GEOMETRY.BIN" --screen discovery

# The CLI:
UG2="cargo run -p gizmo-nfs --features tools --bin ug2 --"
$UG2 info    "$NFSU2_ROOT/CARS/240SX"
$UG2 export  "$NFSU2_ROOT/CARS"  -o out/          # every car: .glb + OBJ/MTL + PNG
$UG2 diff    A.BIN B.BIN                          # chunk-by-chunk comparison
$UG2 dump    "$NFSU2_ROOT/CARS/240SX/GEOMETRY.BIN"
```

Flags worth knowing: `--tab 3d|hex|texture`, `--screen validation|discovery|diff|dictionary`,
`--select <offset>`, `--compare <file>`, and `--shot <png>` (draw a few frames, save the window,
exit — how the interface gets checked on a machine whose compositor will not hand out a screen
grab).

## What is decoded, and what is not

See [`crates/gizmo-nfs/README.md`](crates/gizmo-nfs/README.md) for the per-format status table. In
short: chunk trees, RefPack/QFS and JDLZ, BIGF/VIV, `GEOMETRY.BIN` models, TPK textures (DXT1/3/5 —
HUFF-compressed ones are listed but not decoded), `GLOBALB` car records, and the asset-name hash.

Several of these sub-formats have **no public byte-level spec**. Their offsets and constants are
locked *empirically* against a legally-owned install using `ug2 dump` / `ug2 probe`, never by
assuming an unconfirmed constant — and the commit messages say how each one was pinned down.

## Licence

MIT OR Apache-2.0.
