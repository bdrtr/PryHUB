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
| HUFF decompression | `compression::huff` | ✅ done — order-0 Huffman + clue escape; 52,390 real blobs decode, every one of them stream type `0x30fb` (small 24-bit lengths, no skip words, no delta filter) |
| HUFF **compression** | `compression::huff` | ✅ done — all **52,389** real blobs re-encode and read back byte-exact, at **1.028×** EA's size (JDLZ on the same data: 1.242×). The clue escape is what carries it: order-0 Huffman with runs off is 6.37× EA. `replace_blob` now keeps a blob's own codec, which lifts HUFF-sourced in-place fits from 10/584 to 59/584 in the car packs |
| TPK pixel **encoding** (RGBA8 → on-disk) | `texture::encode` | ✅ done — the half that was missing: DXT1/3/5, uncompressed BGRA and the palettised tags, at the texture's own dimensions and into its own format. The mip chain is taken from the file (`ImageSize`, which a halving chain fills exactly in **2,423 of 2,423** real textures) rather than assumed, and only the image is rewritten, so the blob returns the same length with its header untouched. Round-tripped through the pack over an install: `0x20` comes back identical **436 of 436** and the palettised tags **120 of 120**; DXT3 is identical 378 of 742 and averages 44.9 dB over the 364 that changed, DXT1 47.6 dB over 940, which is what two endpoints per 4×4 block costs. `texture::replace_image` ties it to the write path — encode, try in place, relocate if it will not fit (1,975 of 2,243 fit) |
| TPK texture write-back | `texture::write` | ✅ done — two ways in. **In place** (`replace_blob`) moves nothing and so needs the replacement to fit, and recompresses with the codec the blob arrived in: 1,451 of 2,123 blobs fit (1,393/1,539 JDLZ, 59/584 HUFF — the HUFF share was 10 while JDLZ was the only encoder). **Relocation** (`relocate`) rewrites every blob offset instead, so size stops mattering at all. Relocating all 77 chunked packs with nothing replaced returns **all 54,875 blobs byte-identical**, for 0.36% more file, and a relocated pack has been **read by the game** — all 73 blobs moved, the edited texture visible and the rest correct. Packs written by NFS-CarToolkit relocate too, and are tidier than EA's (descriptor order, contiguous, all JDLZ). The one refusal is `PEUGOT` — raw blocks with no `0x33320002` chunk at all, a rarer variant on one sample — and it is refused with a message saying so rather than guessed at |
| BIGF / VIV archive reader | `viv` | ✅ done |
| Output data contract | `types` | ✅ defined |
| `GEOMETRY.BIN` car models | `geometry` | ✅ done — stride-36 vertices (pos/normal/uv) + u16 indices, validated on real cars |
| TPK textures → RGBA8 images | `texture` | ✅ done — **54,873 of an install's 54,885** decode. Each 24-byte descriptor becomes its own image: whole-file offset → JDLZ **or HUFF** blob (by magic) → embedded `OldTextureInfo` (width/height/format) → DXT1/3/5, uncompressed BGRA, or **palettised** (`0x08`/`0x80`/`0x81` — one layout: a 1024-byte palette located by the header's own two placements, one index byte per pixel). Neither codec nor pixel format is a limit any more; the palettised tags alone were 51,871 images, the whole of every `VINYLS.BIN`. The 12 left are not a format — their embedded header fails its own hash self-check. Note the size: a car's pack is 8.7 MB, a `VINYLS.BIN` is **1.87 GB**, so `Tpk::parse` is for a pack you have measured and `directory` + `decode_one` for one you have not |
| Model **import** (OBJ → mesh) | `import::obj` | ✅ done — the inverse of `export::obj`, and the half that makes it a round trip. An OBJ is not a vertex list: three independent pools and a face names one of each per corner, so a vertex is built per distinct *corner*. `usemtl` is bucketed per object so the runs come out contiguous (which `0x00134B02` needs), polygons are fan-triangulated, and vertex colours are read when present and reported **absent** when not. Verified through Blender 5.2: a whole car exported, round-tripped and written back moved **0 parts** and 5 µm; the same car with one part moved a metre in Blender came back with **exactly that part moved 1.0000 m** and every other one at 0.000000 |
| `GEOMETRY.BIN` **writing** | `geometry::write` | ✅ done, and **the game has read one** — a 240SX whose bonnet was moved a metre in Blender, imported and installed, renders with the bonnet a metre in front of the car and the other 608 parts where they were. `replace_mesh` rewrites a solid's vertices, indices, submesh runs, counts and bounding box, **at a different topology if asked**. All 609 meshes in a real car write back byte-identically; halving a part's triangles shrinks the file, the counts follow, every other part is untouched and all 609 vertex buffers keep their alignment. Three things are computed because the file computes them: the bbox is min−0.01 / max+0.01 (23,292 of 23,299 solids), the buffers are aligned by *where they land* (vertex data on 128, indices and runs on 16 — 18,225/18,225 each, so the file is built, measured and built again until the pads settle), and the two anchors are opposite ends. Underneath, `rebuild` is the repacker plus the one thing it cannot know: `0x00134004` is a per-solid table of **absolute file offsets**, one 24-byte record per solid, and growing any chunk strands it. Measured over 18,230 records: lane 1 is the solid's offset, lanes 2 and 3 its size + 8, lanes 4–5 zero — 18,230/18,230 each, and the records are *not* in file order. A real car rebuilt with a grown vertex buffer keeps every record true where the plain repacker strands 100+, and a no-op rebuild is byte-identical. The importer this row twice said was missing is the row above it — `import::obj`, reached as `ug2 import` |
| Chunk stream **writing** | `repack` | ✅ done — byte-exact rebuild of 113 real files (241 MB); payload replacement with size/alignment fix-up. TPK offset fix-up now sits on top of it in `texture::relocate` |
| Asset-name hash (`bStringHash`) | `hash` | ✅ done — locked against 2,123 real (name, hash) pairs; recovers truncated names by confirming a candidate |
| Chunk-tree comparison | `diff` | ✅ done — paired by position among same-id siblings; changed / resized / one-sided, with the first differing byte |
| Schema discovery (unknown chunks) | `discover` | ✅ done — user-typed stride/columns, exact-divisor candidates ranked by lane consistency, `0x11` filler skipped; re-derives the real vertex layout in a golden test |
| glTF (`.glb`) + OBJ/MTL + PNG output | `export` | ✅ done — pure text/bytes, no filesystem; shared by `ug2 export` and PryHUB |
| `GLOBALB.BUN` car + `CarParts` tables | `globalb` | ✅ done — `CarTypeInfo` (wheels, mass, body box) plus the per-car **handling** record `0x00034600` (`8 + 46×2192` = the chunk exactly): rpm limits, a 9-point torque curve in N·m whose rpm axis is not stored but is recoverable (`torque_rpm`: idle → limiter in eight equal steps, confirmed against the game's own dynamometer), and four gearboxes — stock and three upgrade levels — and four more nine-point torque tables (`torque_gain_nm`, `+0x530`…`+0x5F0`), graduated 34 % / 68 % / 100 % of a per-car maximum in every one of the 31 playable cars, which is what an upgrade ladder looks like. **Writes them back too**: `globalb::edit` puts a named lane into the record in place, and `globalb::install` puts the bundle where the game reads it. Also 12,167 `CarParts`, 49 of 51 attribute keys named, and the 123-colour paint palette; every count checked against its chunk rather than trusted |
| Player profile (fitted upgrades, applied vinyl) | `profile` | ✅ done — which performance products a car has fitted, the per-category totals, and the vinyl on the car (`bStringHash` of its menu name). All locked by diffing a save after each change, twice with the next value written down first |
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
| `ug2 replace PACK --texture NAME --png FILE -o OUT` | put an image back into a texture pack — the one command here that writes an asset file. Re-reads what it wrote before saying it worked |
| `ug2 dump FILE` | the chunk tree of any asset file (or a BIGF/VIV archive's contents) |
| `ug2 diff A B [--all --max N]` | what differs between two asset files, chunk by chunk |
| `ug2 probe CARS/240SX [--matrices]` | the raw solid view: declared counts vs. buffer sizes, mesh-header words, matrix classification |
| `ug2 globalb GLOBALB.BUN [--parts]` | wheel mounts, radius and mass per car; `--parts` for the `CarParts` tables and the paint palette |
| `ug2 tune <root> --car 240SX [--set f=v] [--install]` | a car's handling by name — mass, rpm, both torque curves, the four gearboxes — and, with `--install`, written into the game's own `GLOBAL/` with a `.bak` taken first |
| `ug2 profile PROFILE` | a save's fitted performance products, the per-category totals they sum to, and the vinyl applied to the car |

`dump` and `probe` are the reverse-engineering levers: every unconfirmed offset in this crate
was locked with them against a legally-owned install, never by assuming a constant.

Exports use NFSU2's own coordinates (x = length, y = width, z = height, Z-up — what Blender
reads natively) with each solid's placement applied, so no axis fixup is invented on the way out.
