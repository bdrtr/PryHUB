# gizmo-nfs

A pure, **engine-agnostic** parser for [Need for Speed: Underground 2](https://en.wikipedia.org/wiki/Need_for_Speed:_Underground_2) (2004) asset files, part of the Gizmo Engine workspace.

It reads NFSU2's binary asset containers and hands back plain CPU-side data
(`Vec<f32>` / `Vec<u32>` / `Vec<u8>`) — **no `wgpu`, no `glam`, no renderer coupling**.
Turning that data into engine meshes/materials is the job of a separate integration
layer (a demo binary or an optional `gizmo-nfs-engine` crate), not this crate.

## Status (phased)

| Area | Module | Status |
|------|--------|--------|
| Bounds-checked byte reader | `reader` | ✅ done |
| FourCC helper | `fourcc` | ✅ done |
| Chunk tree (`BinSectionHeader`, high-bit = container) | `chunk` | ✅ done |
| RefPack / QFS decompression | `compression::refpack` | ✅ done |
| JDLZ decompression | `compression::jdlz` | ✅ done — validated byte-exact against a real golden pair |
| JDLZ **compression** | `compression::jdlz` | ✅ done — a real 1.6 MB bundle packs to **29.8%** and reads back byte-exact (EA's own encoder: 30.1%) |
| HUFF decompression | `compression::huff` | ✅ done — order-0 Huffman + clue escape; 52,390 real blobs decode, every one of them type `0xfb30`. There is no HUFF *compressor*, so a rewritten blob goes back as JDLZ |
| TPK texture write-back | `texture::write` | 🟡 in place only — 1,402 of 2,123 blobs (66%) recompress small enough to fit their slot. Split by source codec that is 1,392/1,538 JDLZ but 10/585 HUFF, because JDLZ is the only encoder: 575 of the 721 misses want a HUFF *encoder*, not relocation |
| BIGF / VIV archive reader | `viv` | ✅ done |
| Output data contract | `types` | ✅ defined |
| `GEOMETRY.BIN` car models | `geometry` | ✅ done — stride-36 vertices (pos/normal/uv) + u16 indices, validated on real cars |
| TPK textures → RGBA8 images | `texture` | 🟡 every car decodes, most of the install does not — each 24-byte descriptor decoded to its own image: whole-file offset → JDLZ **or HUFF** blob (by magic) → embedded `OldTextureInfo` (width/height/format) → DXT1/3/5 or uncompressed BGRA. **Codec is no longer a limit; pixel format is.** All 2,123 textures in the 30 `CARS/*/TEXTURES.BIN` decode (a golden test reads all 73 of the 240SX's), but across the install only 3,002 of 54,885 do: the other 51,871 are palettised (`0x08` 25,960, `0x80` 24,071, `0x81` 1,840), which is the whole of every `VINYLS.BIN` |
| Chunk stream **writing** | `repack` | 🟡 foundation done — byte-exact rebuild of 113 real files (241 MB); payload replacement with size/alignment fix-up. TPK offset fix-up (relocation) still to come |
| Asset-name hash (`bStringHash`) | `hash` | ✅ done — locked against 2,123 real (name, hash) pairs; recovers truncated names by confirming a candidate |
| Chunk-tree comparison | `diff` | ✅ done — paired by position among same-id siblings; changed / resized / one-sided, with the first differing byte |
| Schema discovery (unknown chunks) | `discover` | ✅ done — user-typed stride/columns, exact-divisor candidates ranked by lane consistency, `0x11` filler skipped; re-derives the real vertex layout in a golden test |
| glTF (`.glb`) + OBJ/MTL + PNG output | `export` | ✅ done — pure text/bytes, no filesystem; shared by `ug2 export` and PryHUB |
| `GLOBALB.BUN` car + `CarParts` tables | `globalb` | ✅ done — `CarTypeInfo` (wheels, mass, body box) plus the per-car **handling** record `0x00034600` (`8 + 46×2192` = the chunk exactly): rpm limits, a 9-point torque curve in N·m, and four gearboxes — stock and three upgrade levels. Also 12,167 `CarParts`, 49 of 51 attribute keys named, and the 123-colour paint palette; every count checked against its chunk rather than trusted |
| Player profile (fitted upgrades) | `profile` | ✅ done — which performance products a car has fitted and the per-category totals, locked by diffing a save after each purchase |
| World / city (`STREAM*.BUN`, `L4RA.BUN`) | `world` | 🔴 research-frontier |

The crate is **synchronous on purpose**: it is CPU work over byte slices with no I/O to wait for,
so it stays callable from any thread or runtime, and consumers decide where to run it (PryHUB gives
it a worker thread; `ug2` gives it several).

Several NFSU2 sub-formats have **no clean public byte-level spec**; those modules are
built defensively and their exact offsets are locked empirically using the `ug2` tool
(`ug2 dump` / `ug2 probe`) against a legally-owned game install — never by assuming
unconfirmed constants.

## Legal / asset hygiene

This crate ships **no copyrighted game data**. All tests use synthetic byte buffers.
Reading real assets is done at runtime from a user-provided install path. You must own
your copy of the game.

## The `ug2` command-line tool

```bash
cargo run -p gizmo-nfs --features tools --bin ug2 -- <command>
```

| command | what it answers |
|---|---|
| `ug2 info CARS/240SX` | what this car is: parts, the variants it ships (`--kit`/`--hood`/`--light`/`--wide`), dimensions, and its `GLOBALB` wheel record |
| `ug2 parts CARS/240SX [--selected --kit 3]` | every part grouped by customization namespace, or just the ones a configuration selects |
| `ug2 export CARS/240SX -o out/ [--kit 3 --wide 1] [--format glb\|obj\|both]` | the car as a self-contained `.glb` and/or OBJ + MTL + PNG — importable anywhere |
| `ug2 export CARS/ -o out/` | the same for every car in the folder, one subdirectory each |
| `ug2 textures CARS/240SX` | the texture table, and which material run resolves to which image |
| `ug2 dump FILE` | the chunk tree of any asset file (or a BIGF/VIV archive's contents) |
| `ug2 diff A B [--all --max N]` | what differs between two asset files, chunk by chunk |
| `ug2 probe CARS/240SX [--matrices]` | the raw solid view: declared counts vs. buffer sizes, mesh-header words, matrix classification |
| `ug2 globalb GLOBALB.BUN [--parts]` | wheel mounts, radius and mass per car; `--parts` for the `CarParts` tables and the paint palette |
| `ug2 profile PROFILE` | a save's fitted performance products and the per-category totals they sum to |

`dump` and `probe` are the reverse-engineering levers: every unconfirmed offset in this crate
was locked with them against a legally-owned install, never by assuming a constant.

Exports use NFSU2's own coordinates (x = length, y = width, z = height, Z-up — what Blender
reads natively) with each solid's placement applied, so no axis fixup is invented on the way out.
