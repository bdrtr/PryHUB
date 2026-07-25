# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**PryHUB** — a Need for Speed: Underground 2 (2004) asset toolkit for Linux. A Rust workspace with
two crates:

- **`crates/gizmo-nfs`** — a pure, engine-agnostic NFSU2 asset parser. No GPU/graphics types
  (`wgpu`, `glam`), no interface dependencies. It reads NFSU2 binary containers and returns plain
  CPU data (`NfsCar`, `NfsMeshPart`, `NfsTexture` — `Vec<f32>`/`Vec<u32>`/`Vec<u8>`), and ships the
  **`ug2`** command-line tool behind its `tools` feature. Publishable standalone.
- **`crates/pryhub`** — the app: the parser's data as an interface.

### The sibling project

The game — recreating NFSU2 inside the [Gizmo engine](https://github.com/bdrtr/Gizmo) — lives in
its own repository, [`nfsu2-gizmo`](https://github.com/bdrtr/nfsu2-gizmo), and consumes
`gizmo-nfs` as a dependency the same way anyone else would. **Changes here reach it through a
release, not through a shared workspace**, so:

- `gizmo-nfs` is a *published contract*: renaming or narrowing a public item is a breaking change
  for a consumer this repo cannot see. Add rather than reshape when it is close.
- The parser stays engine-agnostic. Anything that needs `glam`, `wgpu` or a scene graph belongs on
  the game's side of the boundary, not here (see the output contract below).
- If you are changing both at once, check out the two repos side by side; `nfsu2-gizmo` patches
  `gizmo-nfs` to `../PryHUB/crates/gizmo-nfs` for exactly that.

## Build & run

```bash
# Parser only — fast, pure, synthetic tests:
cargo test -p gizmo-nfs

# A single test / test file:
cargo test -p gizmo-nfs --test golden_assets
cargo test -p gizmo-nfs geometry_bin_parses     # by name substring

export NFSU2_ROOT="/path/to/Need for Speed Underground 2"
```

### The app (`crates/pryhub`)


The adopted design (`claude.ai/design`, project `8ac61419-…`) as a native egui app: chunk tree ·
hex · 3D · texture · inspector · log · validation, over one open file. It depends on `gizmo-nfs`
(this workspace) and eframe, and on nothing else — no engine, no game crate.

```bash
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/240SX/GEOMETRY.BIN"
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/3000GT/GEOMETRY.BIN" --screen validation --shot out.png
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/RX7/GEOMETRY.BIN" --tab 3d
cargo run -p pryhub -- "$NFSU2_ROOT/CARS/240SX/TEXTURES.BIN" --tab texture
```

The 3D tab renders through eframe's own wgpu device into an offscreen target (egui's pass has no
depth attachment, and a solid drawn without one shows its far side through its near side). It
keeps the file's frame — Z-up, 1 unit = 1 m — and shows the selected solid, or the showroom car
when the selection is not inside one.

`Dışa Aktar` writes **what is on screen**, under `pryhub-export/<car>_<file>/` in the working
directory (there is no file dialog on purpose — see `crates/pryhub/Cargo.toml`), and the log says
the path: the texture tab gives every decoded PNG, any other tab gives the shown model as a
self-contained `.glb` plus OBJ + MTL + the textures it references. The preview pane's `PNG`
button writes just that one image. The writers themselves are `gizmo_nfs::export`, so PryHUB and `ug2 export` cannot drift.

The texture tab is a contact sheet over the car's TPK: the open file when it is itself a
`TEXTURES.BIN`, else the `TEXTURES.BIN` beside it, decoded on first use because `Tpk::parse`
expands all 57–76 images to RGBA8 at once. Thumbnails are downscaled on the CPU and only the
selected image is uploaded full-size (nearest-filtered, so a preview shows texels rather than a
smear). Entries the parser could not decode are **counted out loud** next to the total.

The discovery screen (`--screen discovery`) is the other half of the inspector: pick a chunk, and
it proposes a reading — filler skipped, the best-scoring stride, the lanes typed — then lets you
drag the header, click a candidate stride, or cycle a column's type and watch the table change.
The numbers that say a guess is wrong are on screen: bytes left over, and bytes of each record no
column claims. All of the judgement lives in `gizmo_nfs::discover`; the screen is the table.

The compare screen (`--screen diff`, second file via `--compare <file>` or dropped on the window)
lists what differs between the open file and another one, with the first differing byte offset per
chunk; clicking a row goes to that chunk in the left file. Only differences are listed unless asked
otherwise — "what is different about these two cars" should not arrive as seven thousand lines of
*same*.

The dictionary screen (`--screen dictionary`) lists every hash the open file points at — textures,
material runs, shaders, solids — with the name the file gave it (dimmed when it does not hash back,
i.e. truncated) and a name you can type. A typed name that hashes back gets a **drawn** tick; one
that does not keeps a hollow ring and is stored as a note. Names live in
`$XDG_CONFIG_HOME/pryhub/names.tsv` (`hash<TAB>name`, hand-editable, written on each edit) and the
texture tab prefers them over the file's truncated ones.

`--shot <png>` draws a few frames, writes the window and exits — this machine's compositor will
not hand out a screen grab, so it is the only way to check the interface. `--screen <name>` opens
on a screen other than the workspace, and `--select <offset>` (hex or decimal) preselects a chunk,
which is how a screenshot shows a screen reading something other than the root.

`ug2 info <car>` lists the `KIT##`/`KITW##`/`STYLE##` numbers a given car actually ships, which are
what `--kit`/`--wide`/`--hood`/`--light` take.

### The `ug2` CLI

One tool over the whole parser — inspect a car, or export it. Read-only, ships no game data:

```bash
UG2="cargo run -p gizmo-nfs --features tools --bin ug2 --"
$UG2 info   "$NFSU2_ROOT/CARS/240SX"                 # parts, variants, dimensions, GLOBALB record
$UG2 parts  "$NFSU2_ROOT/CARS/240SX" --selected --kit 3
$UG2 export "$NFSU2_ROOT/CARS/240SX" -o out/ --kit 3 --wide 1   # GLB + OBJ/MTL + PNG
$UG2 export "$NFSU2_ROOT/CARS/240SX" -o out/ --format glb        # just the one self-contained file
$UG2 export "$NFSU2_ROOT/CARS" -o out/ --format glb              # every car, each into out/<CAR>/
$UG2 dump   "$NFSU2_ROOT/CARS/240SX/GEOMETRY.BIN"    # chunk tree / VIV listing
$UG2 diff   "$NFSU2_ROOT/CARS/TAXI/GEOMETRY.BIN" "$NFSU2_ROOT/CARS/TAXI02/GEOMETRY.BIN"
$UG2 probe  "$NFSU2_ROOT/CARS/SENTRA" --matrices     # raw solids: counts, buffers, matrices
$UG2 textures "$NFSU2_ROOT/CARS/240SX"
$UG2 globalb  "$NFSU2_ROOT/CARS/240SX"
```

Pointed at a `CARS/` folder, `export` does the lot: one subdirectory per car, a failed car
reported and skipped rather than aborting the run (but the command still exits non-zero), and
`WHEELS/` expanded into its `GEOMETRY_<BRAND>.BIN` members as `WHEELS_BBS` and kin — the whole
install is 80 models.

`dump` and `probe` are the workhorses for locking an unconfirmed format (they replaced the
old `nfs_dump`/`nfs_vfmt`/`nfs_survey` examples). `export`'s **OBJ** writes NFSU2's own
coordinates (x = length, y = width, z = height, Z-up — Blender reads it natively) with each
solid's placement applied; its **`.glb`** rotates into glTF's mandated Y-up frame, because that
one the format dictates.

### RAM-limited builds

`.cargo/config.toml` caps `jobs = 4` and disables LTO because the dev machine has 13 GB RAM (each `rustc` uses ~1–2 GB). Do not remove this unless building on a higher-memory machine.

## Asset hygiene (important)

**No copyrighted game data ships in this repo.** `.gitignore` blocks `*.BIN`/`*.VIV`/`*.bun`/`*.lzc`.
All unit tests use synthetic byte buffers. Golden tests (`crates/gizmo-nfs/tests/golden_assets.rs`)
read a real install and are **skipped unless `NFSU2_ROOT` is set**, so CI and other machines stay
asset-free. Never commit real assets or hardcode a working install path into committed code.

## Parser architecture (`crates/gizmo-nfs`)

Layered bottom-up; each layer is `&[u8]`-based and independently testable:

1. **`reader`** — `ByteReader`, a bounds-checked byte cursor. The panic-free foundation; every read returns `NfsResult`.
2. **`fourcc`** — printable rendering of 32-bit chunk IDs.
3. **`chunk`** — the universal NFSU2 chunk tree. Almost every asset is a stream of 8-byte-headed sections (`BinSectionHeader { id, size }`, both LE; `size` = bytes *after* the header). Classification by `id`: **high bit set → container** (recurse), **high bit clear → leaf** (payload), **`id == 0` → padding** (skip). Two consumption styles share one core: zero-alloc `walk()` visitor, and a materialized `ChunkNode` tree (`parse`/`find`/`find_all`) whose leaves borrow from the root buffer.
4. **`compression`** — `detect()` picks the codec **by magic bytes, never by extension** (a `.LZC` may be either). RefPack/QFS (magic `10 FB`) and JDLZ (magic `"JDLZ"`). JDLZ also **compresses**: repack needs a writer, and a decompressor is not one. It does not try to reproduce EA's byte stream — two LZ encoders that pick different matches both produce valid files — so it is judged on reading back (proptest over arbitrary bytes, plus a real 1.6 MB bundle) and on ratio: 30.5% where EA's own encoder gets 30.1%.
5. **`viv`** — BIGF/VIV archive extraction.
6. **`geometry`** — `parse_geometry()`: `GEOMETRY.BIN` → `Vec<NfsMeshPart>`. Solids without a mesh (mount/dummy points) are skipped.
7. **`texture`** — `Tpk::parse()`: `TEXTURES.BIN` (TPK) → per-texture RGBA8 images. Each texture is independent: its 24-byte descriptor (`0x33310003`) gives hash + **whole-file** offset + compressed/decompressed size; the blob is decompressed by magic (JDLZ) and an embedded `OldTextureInfo` header near its tail gives width/height/format, which `dxt` then decodes (DXT1/3/5) or unpacks (RGBA). HUFF-compressed textures are listed in `entries` but absent from `textures` — **counted, never silently dropped**. See its module docs for the byte-level table.
8. **`placement`** — what a solid's local matrix *means*: a placement to apply, or a pose already baked into the vertices (`should_place`). Format semantics, so every consumer (the app, the CLI exporter, the game's engine layer) decides it the same way.
9. **`parts`** — **pure policy**: which material group a name is (`group_of`), what its `KIT##`/`KITW##`/`STYLE##` token says, and which parts make up a configuration (`select_car`). Lives here so the `ug2` CLI, the app and the game (a separate repo) all select identically; the game re-exports it as `nfsu2::parts`.
10. **`inspect`** — a chunk's bytes read back as labelled fields, each with the offset it came from (`model`). What an inspector pane draws; it reads through `geometry::format` so a viewer cannot drift from the parser about what a file says.
11. **`validate`** — the checks a person would run by hand: stride, bbox, normals, index range, chunk bounds. Every rule records **what it examined**, so "no findings" is never confused with "nobody looked".
12. **`discover`** — the inverse of `inspect`: read an *undecoded* chunk through a `Schema` (header + stride + column kinds) a person typed. It carries no per-chunk knowledge, only the arithmetic that cracks layouts in this format: `leading_filler` (the `0x11` run that is not part of the records), `stride_candidates`/`ranked_candidates` (strides that divide exactly, scored by whether their *lanes* hold a consistent kind of value — a divisor of the true stride mixes fields between lanes and scores badly, a multiple ties, so the answer is the best-scoring **shortest** stride), `stride_for` (`size / n`), and `guess_columns`. A golden test asserts `propose()` re-derives a real car's stride-36 vertex layout from bytes alone.
13. **`hash`** — `bStringHash`, the function NFSU2 names its assets by: `h = h * 33 + byte` from `0xFFFFFFFF`. **Locked empirically, not from a spec**: over one install's 2,123 TPK (`DebugName`, hash) pairs it reproduces the hash for every name that fits the 23-character name field and fails only for names of exactly that width — i.e. only where the input is known to be truncated. Two uses: name → key, and *confirming a guess*, which is the only way a truncated name's tail comes back (`240SX_DOORLINE_WIDEBODY` and its `_MASK` twin arrive under one truncated name; the hash tells them apart).
14. **`diff`** — two files, chunk by chunk: `Same` / `Changed` (same size, different bytes, with the first differing offset) / `Resized` / `OnlyLeft` / `OnlyRight`, and a container is `Changed` exactly when something inside it is. Chunks are paired **by position among siblings of the same id** — this format's trees are ordered, and any cleverer pairing would silently re-order parts and invent differences. Not a byte diff: after one edit every later offset has shifted, so bytes would be a wall of noise.
15. **`export`** — parsed data back out as files other tools read: `obj` (OBJ + MTL text), `gltf` (a self-contained `.glb`, images embedded — behind the `png` feature), `material` (`MaterialPlan`: which `newmtl`/glTF material a run resolves to, and the textures that implies), `png_name`/`png_bytes`. Pure — it returns text and bytes and never touches the filesystem, so `ug2` and PryHUB write the same car from the same code. **glTF is the one place a frame is converted**: the format *defines* +Y up / −Z forward, so `gltf` rotates `(x,y,z) → (−y,z,−x)` (a rotation, not a mirror) and leaves UVs alone, since glTF's UV origin is DirectX's. OBJ keeps the file's own frame and flips V.
16. **`repack`** — the write path: `rebuild(bytes, edits)` reassembles a chunk stream, recomputing container sizes and alignment padding, replacing named leaf payloads. **Measured, not assumed**: every chunk size and offset in a real install is a multiple of 4; `0x80134010` (SolidObject) starts on a **128**-byte boundary 18,230 times out of 18,230, padded with an `id == 0` chunk only when needed; a 4-byte alignment debt is unpayable (a header is 8) so the files pay 132; `0x80034020` aligns to 64. An `id == 0` chunk is *not* always padding — BUS carries one of 1,332 bytes with a non-zero byte inside — so a gap is only re-derived when it is exactly what the rule would produce, and copied otherwise. A golden test rebuilds 113 files byte-for-byte.
17. **`types`** — the engine-agnostic output contract (see below).

The top-level `decompress_file()` is one of the few functions that touch the filesystem; everything downstream is pure `&[u8]`.

### Two hard invariants

- **Panic-free parsing.** The crate is `#![forbid(unsafe_code)]`. Input is always untrusted: every read is bounds-checked and returns an `NfsError`; no parse path may panic, `unwrap`, or allocate from an unchecked size field. `tests/no_panic.rs` enforces this with proptest against arbitrary/adversarial bytes — **any new parser must uphold it.**
- **Empirically-locked formats.** Several NFSU2 sub-formats have no public byte-level spec. Their exact offsets/constants are locked *empirically* using `ug2 dump`/`ug2 probe` against a legally-owned install, never by assuming unconfirmed constants. When touching format code, document offsets the way `geometry/mod.rs` does (chunk-ID map + stride/field-index constants) and validate against a real car. The objective correctness check for vertex layouts is `NfsMeshPart::indices_in_range()` (a correct layout yields all in-range indices).

### Output contract (`types`)

Pure-data structs, no `glam`/`wgpu`. Geometry is **indexed** and transforms are stored **as-in-file** (row-major, original handedness). Expanding indices to a flat vertex list and any coordinate-system fixups are deliberately the **integration layer's** job, not the parser's — e.g. the game's `geom::remap()` converts NFSU2's Z-up frame to Gizmo's (and `ug2 export` deliberately does not, writing the file's own frame). `serde` derives on all output types are gated behind the optional `serde` feature.

## Conventions

- Code comments and Cargo.toml notes are frequently in **Turkish** — match the surrounding language when editing a file.
- **Part names are truncated** to a fixed-length field in `GEOMETRY.BIN`. Long names lose their tail: `..._HEADLIGHT_LEFT_LOD_A` arrives as `..._HEADLIGHT_LEFT_` (LOD letter gone, so two LODs share a name — disambiguate by triangle count) and `..._SIDE_MIRROR` as `..._MIRRO`/`..._MIRR`. Match shortened stems (`MIRR`), never assume the full word survives. A truncated name can be *recovered* rather than guessed: `gizmo_nfs::hash` hashes a candidate and the file's own hash says whether it is right. `NfsTexture::name_is_whole()` asks whether the stored name survived the cut, and `NfsTexture::is_mask()` answers "is this another texture's `_MASK` companion" **through** the truncation — one install hides 56 such masks across 29 cars, all fully opaque, and binding one as a diffuse map is a black panel. The game's skin matching (`car::skin`) filters on that proof rather than on how transparent an image happens to be. Cars also carry `STYLE00..STYLE14` purchasable part variants and `KIT01+` body kits alongside the default `BASE`/`KIT00`; render only the default set or variants overlap.
- Status: the toolkit's phases 1–4 are done (read · visualise · export · discover); repack (writing files back) is deliberately last, because TPK stores **absolute** offsets — changing one texture means writing a *compressor*, recomputing every later offset and keeping the alignment. The TPK texture format is decoded (per-texture DXT1/3/5 + RGBA); what is left there is **HUFF-compressed** textures, which are listed but not decoded. See `crates/gizmo-nfs/README.md` for the per-format status table.
