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

### Architecture: what runs where

Three layers, and the boundary between them is the point:

1. **`gizmo-nfs` is synchronous and pure.** Almost everything it does is CPU work over `&[u8]` with
   no waiting in it, so there is nothing for `async` to manage: it would infect every call with
   `.await`, put a runtime inside a crate whose value is having no dependencies, and make nothing
   faster. Kept sync, it is callable from any runtime, any thread, and any test.
2. **The app never does that work on the frame the user is looking at.** `pryhub::jobs` is one
   worker thread and two channels; opening a file, decoding a pack and exporting are requests, and
   results arrive in `collect_jobs()` at the top of the next frame. The document is immutable once
   parsed and shared as an `Arc`, so a job reads it without copying or locking, and results carry
   the identity of what they were computed *for* — a decode that lands after the user opened another
   file is dropped rather than applied to the wrong document. `--shot` waits for the worker to go
   quiet, or it would photograph an empty window.
3. **The CLI parallelises over independent work.** `ug2 export CARS/` runs on `available_parallelism()`
   threads capped at 8 (each worker holds a car's bytes plus its parsed geometry — tens of MB), with
   `--jobs N` to override. Output stays in **car order**, not completion order, so a run is
   reproducible: 80 models in 0.7 s where sequential took 2.3 s, and two runs are byte-identical.

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
when the selection is not inside one. Under it is the design's ground grid: 1 m cells over ±8 m at
`z = 0`, fixed rather than fitted to the model, because it is a *scale* — a grid that resized itself
with the selection would show a wheel and a whole car on the same ten squares. `Wireframe` swaps the
surfaces for their edges. Both are one extra pipeline: the same vertex layout at `LineList`
topology, with the line's ink carried in the slot a surface uses for its normal, so a grid line and a
wire edge can be different colours in one pass without a second uniform.

The tab's **paint row** is the game's own palette, read from `GLOBALB.BUN` by
`globalb::carparts` — 123 colours, and the only place a car's colour is written down at all. Picking
one replaces the `Grp::Paint` runs' group colour and **nothing else**, so the glass stays glass and
the tail lights stay red; the mesh is rebuilt when the choice changes, the same way it is when the
texture pack lands. The bundle is 8 MB, so reading it is a job (`jobs::Request::Palette`) asked for
only while that tab is open, found from the file's own path (two levels up to `GLOBAL/`) or from
`NFSU2_ROOT`. A file opened from outside an install simply gets no paint row.

`Scope::Build` writes exactly what the assembly tab has mounted — the car on screen, minus whatever
was taken off it, as a self-contained `.glb` with its textures embedded. The mounted set is resolved
into part indices **when the button is pressed**, not read on the worker: the export is about the
build the user was looking at, and they may go on toggling while it writes.

The **assembly tab** (`--tab assembly`) is the build: `select_stock_car`'s parts as a checklist,
each row a component with its triangle count, and the viewport re-uploading as they are switched off
and on. The *selection* logic is not written twice — the list is `gizmo_nfs::parts`' answer, and this
screen only offers to take things out of it. Two things the design draws that this does not: it
groups `_LEFT`/`_RIGHT` into one `×2` row because those are genuinely two parts, and it does **not**
write `×4` on a wheel — a `GEOMETRY.BIN` carries one wheel mesh and the four corners come from
`GLOBALB.BUN`, so counting four here would be counting something the file does not contain.

The preview is **skinned**, and by the same rules the exporter uses. Each part's `0x00134B02`
material runs become one draw apiece; a run whose hash resolves in the TPK gets that texture, and one
that does not gets its material group's colour from `gizmo_nfs::export::material::group_colour` — the
same value `ug2 export` writes into the MTL, so the viewport and the exported car are the same car.
Both arrive as a bound texture (the colour as a 1×1), so there is no branch in the shader and no
second pipeline. Two things worth knowing before reading a car's paint off the screen: only 3 of the
240SX's 30 materials resolve to a texture — **NFSU2 does not texture a car's body**, it paints it with
a colour chosen in-game — and `group_colour` hands out shading *coefficients*, so they are encoded
into the sRGB texture rather than stored raw, or the sampler linearises them a second time and a
silver car comes out navy.

`Dışa Aktar` opens the export dialog (`screens/export_dialog.rs`, `--screen export`): scope
(selection / **build** / whole file), model format (glTF · OBJ · both) and target folder, with the primary
button stating what it is about to write. It used to fire on the spot and decide what it meant by
looking at which tab was in front — a file whose contents depend on something the user was not
thinking about is not an answer. Only what the tool can keep is offered: there is no DDS writer in
`gizmo-nfs`, so that row is drawn in the design's disabled treatment rather than dropped or, worse,
silently ignored. Files land under `pryhub-export/<car>_<file>/` in the working directory (there is
no file dialog on purpose — see `crates/pryhub/Cargo.toml`) and the log says the path. The preview
pane's `PNG` button still writes just that one image, with no dialog — it is a one-click action on a
thing already on screen. The writers themselves are `gizmo_nfs::export`, so PryHUB and `ug2 export`
cannot drift.

`Değiştir…` on the preview pane is the tool's **write path**, and the first thing it writes that is
not its own output. It does not write: it **stages**. The chooser's answer is checked against the
texture's dimensions there and then — through `export::png_size`, which is the header and not the
pixels — and either joins the pending set or says why it cannot, so nobody stages six images and
learns at the end that the second was never going to fit. One image per texture; a second replaces
the first.

Staged edits show as the design's CARP vocabulary, which is where the prototype puts its only write
affordance: an accent dot on the tile, `● n change` beside the count, and a `Save`. (Its **words**
are not borrowed — `cp_save` reads "Save CARP" and said so on screen until it was noticed. Borrow a
treatment, not a sentence about another screen. The dot is *drawn*, for the reason the validation
marks are: the bundled font has no `●` and a missing glyph is a box, which it was.)

Save opens `screens/replace_dialog.rs` (`--screen replace`, with `--stage <png>` to reach it in a
screenshot): what goes in, then the one choice that is not a preference — a copy under
`pryhub-edit/`, or **over the game's own file** with a `.bak` written first, atomically, once. A copy
is the default, and the resolved path is on screen before the button is pressed rather than in the
log after.

**The set is why it is a set.** A write reads the pack from disk, so replacements written one at a
time into a copy each started from the original and the second discarded the first; and each one
rewrote an 8.7 MB file on its own. `texture::replace_images` takes them all, encodes against one
input and relocates once if any single one needs it. A set is also refused *whole* — every image is
checked before any is encoded.

The worker **decodes the pack it just built and pulls every edited texture back out of it before
writing anything**: nothing else in this program can check its own output, but a pack can be handed
straight back to the parser. When the write lands on the *open* document — not the pack beside it —
the document is reopened rather than merely re-decoded, carrying the selection and the replaced
texture across: `Doc::bytes` is the snapshot taken at open, so a re-decode alone reads the pre-write
image, and the sheet would redraw the old pixels under a log line saying it had written new ones.

The texture tab is a contact sheet over the car's TPK: the open file when it is itself a
`TEXTURES.BIN`, else the `TEXTURES.BIN` beside it, decoded on the worker because a whole pack
becomes RGBA8 at once. Thumbnails are downscaled on the CPU and only the selected image is uploaded
full-size (nearest-filtered, so a preview shows texels rather than a smear).

It is **virtualised** and it is **budgeted**, and both only became true when the palettised formats
started decoding. A car's pack is 73 images; `CARS/*/VINYLS.BIN` is 1,786, and it used to decode to
nothing at all. So the sheet was the one long list in the crate not using `show_rows` — harmless at
73 tiles, and 1,786 CPU downscales plus 1,786 `load_texture` calls in one frame at the real number,
because egui clips the painting and still runs the loop. And `doc::decode_pack` decoded every entry,
which for that pack is **1.87 GB** held resident behind an `Arc`; it now stops at `DECODE_BUDGET`
(256 MB) and reports what it did not read. What comes back is the longest *prefix of file order
whose pixels fit the budget*, which is a property of the file and not of the threads: the workers'
stopping point is interleave-dependent (up to eight claims are in flight past the byte that crosses
the line), so it is used only to stop, and the cut is recomputed afterwards by walking the decoded
entries in index order. A handful of decodes past it are discarded — the price of not serialising
the workers to find out where it falls — and two runs on one file agree.

The sheet says **two** numbers next to the total, and keeping them apart is the point: entries the
parser *could not decode* (the file's shortfall) and entries that were *not read* (this program's
budget). Added together they would read as one count of broken textures, and the second kind is not
broken. The 3D tab, correspondingly, no longer asks for textures when the file has no parts — a
`VINYLS.BIN` has none, so every image would have been decoded to be looked up zero times.

The **CARP screen** (`--screen carp`, second in the nav as the design places it) is the design's
car-parameter table — nine `[::SECTION]`s and 39 parameters over four upgrade columns. It is drawn in
full and then tells the truth cell by cell, because **NFSU2 ships no `CARP.BIN`**: `find -iname
'*carp*'` over an install returns nothing, CARP being the older NFS engine's format. What this game
does carry is `GLOBALB.BUN`, and the screen reads it. A cell with no answer is an em dash in the
design's disabled treatment, the Save/Revert buttons are drawn disabled (there is nothing to write
back to), and the panel note says why. Sections with nothing behind them are listed rather than
dropped: "the format has an aero section and this game does not store one" is a different statement
from "there is no aero", and only the first is true. Tests assert the 9 sections, the 40 rows and the
exact list of rows claiming a source — the count one already caught a torque curve written with seven
rpm steps where the design generates eight.

**Most of the rest is now read.** `GLOBALB.BUN`'s `0x00034600` is a per-car physics record:
`8 + 46 × 2192 == 100,840`, the chunk's size exactly, one record per `CarTypeInfo` car and in the
same order, each carrying its name at `+0` and again at `+32`. `globalb` reads it into
`CarHandling` — engine limits at `+0x300` (240SX: 800 / 6500 / 7000), a **9-point torque curve** at
`+0x310` in N·m that rises to an interior peak and falls again in all 46 cars (240SX peaks at 216),
four 64-byte gearbox blocks at `+0x2C0`/`+0x460`/`+0x4A0`/`+0x4E0` which are the design's STOCK and
three upgrade columns, the rear-drive fraction, the body box and tyre widths. The 240SX reads
`3.321 / 1.902 / 1.308 / 1.0 / 0.9` on a 4.083 final drive and **gains a sixth gear** at level 3;
the G35 ships a six-speed. `ug2 globalb <car> --handling` prints the lot.

That takes the screen from 2 filled rows to **24 of 40**, and the gearbox is the only section whose
four upgrade columns fill — it is the only thing the record stores four times. Everything else it
stores once, shown under `STOCK` and left blank under the upgrades, because repeating a number across
four columns would read as "this upgrade changes nothing", which the file does not say.

Three sections stay empty and that is a **measurement, not a gap**: `[::AERO]`, `[::BRAKES]` and
`[::STEERING]` are not in this game's files. The one brake-shaped triple is exactly zero for all 15
traffic vehicles; the only steering angles are a global ±43 identical in all 46 records, so a row fed
from either would print the same number for a bus and a Skyline. `torque_scale` has no lane because
the curve is absolute rather than normalised; `shift_time`'s only candidate is one constant shared by
45 of 46 cars.

The **one deliberate departure**: the design's torque section is eight rows labelled by rpm, from its
own `rpmSteps`. The file holds nine magnitudes and **no rpm axis** — every 4-aligned lane of all 46
records was swept for a nine-wide increasing run and there is none. So the rows are the file's nine
points, unlabelled. Putting eight invented rpms on nine real numbers is the one thing this screen
exists not to do, which is also why its empty cells say "not in this game's files" rather than
"not located" — for one commit they said the wrong one of those, while the record was being found.

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

The file's health is **one tally** (`Doc::health`), drawn twice: as the top bar's `⚠ n ✓ n` pill and
as the validation screen's two chips. Both count *rules* — a rule that found something is a warning,
a rule that read something and found nothing is a pass, a rule that read nothing is neither, because
"I did not look" is not a result. It was counted at each of the two, and they disagreed: the corner
counted rules while the screen counted **findings**, so one unhappy rule with forty findings read
`⚠ 1` in the corner and `40 warnings` on the screen that corner is a link to. The finding count
belongs on the rule's own card, which is where it now stays.

### Checking the port against the design

The design is **not in this repo** — it is a drop you supply, and `.gitignore` keeps it out. Put it
at the repo root as `STRUKT NFSU2 Asset Tool/`, where `PRYBAR.dc.html` is the working prototype and
`_ds/modernist-…/styles.css` its token sheet. Everything measured below was measured off that drop;
without it the numbers in this section are still the contract, there is just nothing to diff against.
It renders in any browser, so a screen can be compared side by side with the real thing rather than
from memory:

```bash
(cd "STRUKT NFSU2 Asset Tool" && python3 -m http.server 8731 &)
google-chrome --headless --window-size=1360,840 --virtual-time-budget=5000 \
  --screenshot=design.png http://127.0.0.1:8731/PRYBAR.dc.html
```

Its initial state is one literal (`this.state = { screen:'workspace', lang:'tr', density:'compact', … }`)
and the `data-props` defaults override it on mount, so a copy with both edited gives any screen at any
language and density — which is how every measure below was checked rather than guessed.

Two things that cost real fidelity when read carelessly:

* **`box-sizing: border-box`.** A page's `max-width:880px` with `padding:34px` is **812 px of
  content**. Passing the CSS number straight through as a content width made four of the five pages
  68 px too wide. [`widget::page`] now takes the design's own number and its padding and does the
  subtraction itself.
* **The `D` table is the density scale**, and it matches `theme::Density` exactly — row height
  22/27/33, tree 242/266/290, inspector 288/314/342, log 150/176/202. Anything that does not move
  with it is a bug, not a choice.

Deliberate departures, all of them because the port is a program and the prototype is a picture:
the settings menu instead of bare `TR`/`EN` + `S`/`M`/`L` (a setting nobody can find is not a
setting), the path field on the welcome screen (there is no file dialog — see `Cargo.toml`), the
export dialog's disabled DDS row (no writer exists), the validation card's "n chunks read" line
(a rule that read nothing is not a pass), and the discovery screen's stride candidates and
left-over byte count (the numbers that say a guess is wrong).

### The mark

`crates/pryhub/assets/logo.png` — the project's own logo, 506 × 165 with its own alpha, embedded with
`include_bytes!` (`logo.rs`) so a single copied binary keeps its face. It leads the wordmark in the top
bar at 22 px and on the welcome screen at 52 px, in its own blue: it predates the design system, and
recolouring someone's mark to fit a palette is not a port decision to take quietly.

### Things the port promised and did not do

Found by using it rather than by reading it, which is the only way these ever turn up:

* **The tree's caret was decorative.** `app.collapsed` was declared, cleared on open, and read in two
  places — and written by *nothing in the whole crate*. The triangle was painted, so the tree looked
  foldable and was not. The caret column is now its own hit target (a double-click on a container
  folds it too), and `tree::visible_rows` is a pure function with tests, because "a fold hides its
  subtree and stops at the next sibling" is the behaviour, not the painting.
* **"Drop a file or click" did not answer a click.** There is still no file-dialog crate here and
  `Cargo.toml` still says why; `picker.rs` runs whatever chooser the desktop already has
  (`zenity`/`kdialog`/`qarma`/`yad`) as a subprocess, on its own thread because it does not return
  until the user decides. With none installed the click puts the caret in the path field — which is
  then the only way in and should at least be pointed at.
* **Three full pages could not scroll.** Every screen in the design is `position:absolute; inset:0;
  overflow:auto`; discovery, compare and the dictionary had no page-level scroller at all, so
  whatever sat under a long table — `+ Add field`, the legend, the whole add-a-hash row — was cut off
  by the panel with no way to reach it. They scroll now, and each inner table takes a bounded height
  that leaves room for what follows it **and for the page's own bottom padding**, which is the 40 px
  that was putting the dictionary's add row under the status bar.
* **The compare screen's file row never opened a chooser.** The picker was wired into the welcome
  screen and nowhere else, while this row's own hint still read "drop a file or click". It has a
  `Browse` button beside `Open` now — drawn only where `picker::available()`, because a button that
  reaches for a chooser the machine does not have is worse than no button — and `app.picking` carries
  the `Side` that asked, so the answer lands on file B rather than replacing file A.
* **The discovery table escaped its page.** The design puts the whole table in an `overflow:auto`
  box; here the header's cells were laid out one after another with nothing to stop them, so a
  stride with a dozen lanes grew the box through the page's right edge and out of the window. The
  box is capped at the column now and scrolls inside it — and the header is drawn *after* the body
  so it can be offset by what the body is showing, which is why it is reserved first and filled last.

### The design system, in two files

`theme.rs` holds the design's **values** and `widget.rs` the **shapes** built from them. The split
matters because a screen that hand-rolls its own version of either drifts from the rest by a pixel or
a shade, and twelve screens drifting is what a port looks like when it is not one.

* `theme::token` — the palette verbatim from `styles.css`, plus the two widths the whole system's
  rhythm rests on: `RULE` (2 px, where a *region* ends) and `HAIRLINE` (1 px, where a *control* is
  outlined). `theme::muted(n)` is the design's `color-mix(text n%, transparent)`; `shadow::{SM,MD,LG}`
  its elevation scale, tinted with the palette's darkest neutral rather than egui's black.
* `theme::font` — the `h1`…`h6` ladder (42/32/25/20/16/13) and the mono/body roles, so a screen asks
  for a *step* rather than for a number it picked. `theme::tracked()` reproduces letter-spacing, which
  egui's styles cannot express but `epaint` carries per text **section**: `extra_letter_spacing` is
  added before every glyph that has a predecessor *in its own section*. So it is **one section for the
  whole label** — written as one section per glyph, as it first was, no glyph ever has a predecessor
  and the spacing is applied exactly zero times, which set every tracked label in the interface flush
  while the code said otherwise. A test asserts the section count, because that is the whole mechanism.
* `theme::Density` — the design's own `D` table: row height, the three text sizes, and the panel
  measures (tree 242/266/290, inspector 288/314/342, log 150/176/202). `apply()` leaves the current
  setting in the context and `theme::density_of(ctx)` hands it back, so a *widget* can ask for a
  measure egui's `Style` has no field for: `widget::input` took its height from `interact_size`
  (the **control** height, 22/24/28) clamped to 28, which came out a constant 28 px at all three
  settings — the one control the size switch left standing still.
* `theme::mark` / `theme::icon` / `theme::draw` — everything the design draws rather than types:
  the validation marks, the two toolbar icons transcribed from its Lucide paths, the dashed drop
  outline, the accent edge, the 2 px rule, the transparency checkerboard.
* `widget` — the controls: `segmented` (the bordered box of touching buttons, with the accent block
  *sliding* to the selection), `action` (a `.btn` with a drawn icon), `tag`, `card`, `caption_strip`,
  `screen_header`, `page`, `input`, `note_box`, `button_primary`/`_secondary`, `swatch`.

### Settings

Top right, in a menu rather than as bare `TR` / `S` buttons in the corner — those were legible to
whoever wrote them and to nobody else, and a setting that cannot be found is not a setting. It holds
what is true of the *program* rather than of a file: language (listed in its own language), size
(Small / Medium / Large — the `theme::Density` scale), and the two links worth having (README as
help, and the repository).

Defaults on a fresh install: **English** and **Medium**. English because someone who owns the game
and finds this tool should not meet a window in a language they did not choose (the source's comments
stay Turkish); Medium because the design's own compact setting is right once the tool is familiar and
cramped for a first look. Choices persist in `$XDG_CONFIG_HOME/pryhub/settings.tsv` — `key<TAB>value`,
hand-editable, beside the hash dictionary — because a panel whose choices are forgotten when the
window closes is a switch, not a setting.

Making English the default exposed something: the log's lines were built as Turkish sentences at parse
time. A `Note` now carries a `NoteKind` — what happened, plus its numbers — and the panel renders it
in whichever language is on, so switching language switches the log too. The parser's own findings
stay in its words (English): a dependency-free library about byte-level facts speaks one language, and
the log shows both.

It exposed something else, in every screen that counts something: **English agrees a noun with its
number and Turkish does not.** A fresh install's first window said `1 warnings`, `1 findings`,
`1 chunks read`, `4 hash`. A counted noun is now two strings (`i18n::Counted`, reached as
`t.val_findings.of(n)`) and the count picks the form; Turkish answers with the same word twice
(`same("bulgu")`) rather than the code branching on which language it is in — a rule that lives in one
language's table does not have to be remembered by the ten screens that draw numbers.

### Two logs, and why they are not one

* **The log panel** (`doc::Note`) is what the *file* said: a chunk that would not parse, a solid that
  yielded no part, a rule's finding, where an export went. It is interface, it is in the interface's
  language, and every line names a chunk so it can be clicked.
* **`logging.rs`** is what the *program* did: job lifecycle and durations, frame timings, the GPU
  backend's opinions. Nobody using the tool should have to see it. Levels come from `PRYHUB_LOG` —
  `PRYHUB_LOG=info`, or per-target `PRYHUB_LOG=warn,jobs=debug,frame=trace` with longest-match
  precedence — default `warn`, so it is silent unless something is wrong.

It uses the `log` facade rather than an in-house macro because that is the interface `wgpu`, `winit`
and `eframe` already speak: installing one sink makes their diagnostics arrive in the same stream at
the same levels, which is exactly what is wanted the day a GPU refuses a surface. **The parser takes
no logging dependency at all** — it *returns* its findings (`NfsError`, `validate::Report`,
`Skipped`), so it stays usable from anywhere and testable without a logger. `ug2` likewise has none:
its output *is* its report, and it has no interactive loop to instrument.

### Performance, as measured

The hot paths and what they cost on a 240SX (7.6 MB, 609 parts, 73 textures):

| | before | after | how |
|---|---|---|---|
| `parse_geometry` | 9.2 ms | **1.8 ms** | vertices and indices read out of a fixed-size `[u8; 36]` / `chunks_exact(2)` instead of a bounds-checking cursor — the range is validated once, so nine checks per vertex bought nothing (and the array indices are provably in range, so there is no panic path either) |
| `jdlz::compress` | 27 MB/s | **68 MB/s** | `match_len` compares eight bytes at a time, with the first differing byte found from the XOR's trailing zeros. Output is byte-identical: same 463 KB |
| `jdlz::decompress` | 375 MB/s | **565 MB/s** | a non-overlapping back-reference is `extend_from_within` (a memcpy) rather than a byte loop; overlapping ones still go byte by byte, because each byte reads one just written |
| pack decode (app) | 22.7 ms | **5.8 ms** | `Tpk::directory` + `decode_one` + `from_decoded` let the *caller* spread the textures over threads. The library still spawns none — that decision belongs to whoever called it |
| file open (app) | 23.8 ms | **17.5 ms** | the geometry pass above; the remaining 7 ms is reading 7.6 MB off disk |
| golden suite | 10.5 s | **4.1 s** | the compressor, mostly: it runs over 2,123 blobs |

None of it changed a byte of output, and that is checked rather than asserted: fingerprints over every
decoded pixel (2,123 textures) and every parsed vertex, normal, UV and index (4,058,782 vertices
across 18,225 parts) are identical before and after, and all 80 exported models stay byte-for-byte
the same.

**Smoothness is measured, not felt.** `PRYHUB_LOG=frame=trace` prints each frame's cost split into
jobs / bars / screen. It is how the interface went from 7–9 ms a frame to under 0.5: the tree panel was
drawing all 7,246 rows every frame (egui clipped the pixels but did the work), the discovery screen
was copying the selected chunk's payload — 7.5 MB with the root selected — sixty times a second, and
its candidate scoring reran while nothing changed. The rules that came out of it: **virtualise any
list that can be thousands long** (`show_rows`, and set `item_spacing.y` *before* it or the row
pitch is wrong and the list stops short), **borrow from the `Arc<Doc>` rather than copying**, and
**cache anything keyed by a selection that has not moved**.

Animations are `egui`'s own, at one shared duration (`chrome::MOVE_TIME`, 120 ms): the nav and tab
underlines *slide* between items with the label ink crossfading, the tree's selection wash fades in,
and thumbnails fade up as they arrive. A job that can count reports progress and gets a bar; one
that cannot gets a spinner — a moving spinner, because a static "open…" is indistinguishable from a
hang.

`--shot <png>` draws a few frames, writes the window and exits — this machine's compositor will
not hand out a screen grab, so it is the only way to check the interface. `--screen <name>` opens
on a screen other than the workspace, and `--select <offset>` (hex or decimal) preselects a chunk,
which is how a screenshot shows a screen reading something other than the root. Both are *pending*
requests applied when the parse lands — opening is a job, so setting them at startup would otherwise
be overwritten by the document's own defaults a frame later. `--shot` also waits 250 ms after the
worker goes quiet, so the animations have arrived and two runs of the same command produce the same
picture.

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
$UG2 profile  ~/.../AppData/Local/"NFS Underground 2"/<name>   # fitted products + totals
$UG2 replace "$NFSU2_ROOT/CARS/240SX" --texture 240SX_BADGING --png new.png -o out.BIN
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
4. **`compression`** — `detect()` picks the codec **by magic bytes, never by extension** (a `.LZC` may be any of them). RefPack/QFS (magic `10 FB`), JDLZ (magic `"JDLZ"`) and HUFF (magic `"HUFF"`, order-0 Huffman with a "clue" escape — 52,390 of an install's 54,873 texture blobs, and all of them stream type `0x30fb`, i.e. 24-bit lengths with no skip words and no delta filter; the 32-bit-size, skip-words and delta-filter branches are transcribed rather than locked, and the module says so). All three decompress and **two compress** — JDLZ and HUFF; RefPack still has no encoder. Neither tries to reproduce EA's byte stream. It does not try to reproduce EA's byte stream — two LZ encoders that pick different matches both produce valid files — so it is judged on reading back (proptest over arbitrary bytes, plus a real 1.6 MB bundle) and on ratio, which matters because a texture is written back *in place*: 29.8% where EA's own encoder gets 30.1%, thanks to lazy matching.
5. **`viv`** — BIGF/VIV archive extraction.
6. **`geometry`** — `parse_geometry()`: `GEOMETRY.BIN` → `Vec<NfsMeshPart>`. Solids without a mesh (mount/dummy points) are skipped.
7. **`texture`** — `Tpk::parse()`: `TEXTURES.BIN` (TPK) → per-texture RGBA8 images. Each texture is independent: its 24-byte descriptor (`0x33310003`) gives hash + **whole-file** offset + compressed/decompressed size; the blob is decompressed by magic (JDLZ **or HUFF** — a pack mixes both) and an embedded `OldTextureInfo` header near its tail gives width/height/format, which `dxt` then decodes (DXT1/3/5) or unpacks (uncompressed BGRA). A texture that does not decode stays in `entries` and is absent from `textures` — **counted, never silently dropped**. Neither codec nor pixel format stops one any more: **54,873 of an install's 54,885 decode**, up from 3,002. The 51,871 palettised ones (`0x08` 25,960, `0x80` 24,071, `0x81` 1,840) are **one layout**, which is why one arm reads all three — a 1024-byte palette and one index byte per pixel — and what the tag says is only how much of the palette is populated: no `0x80` texture in 24,071 ever indexes above 15, no `0x81` in 1,840 above 63, where `0x08` reaches 255. The palette is found by the header's own two placements (`+0x10` minus `+0x0C`) and **not** by `ImageSize`, because there is no constant to add: the palette starts 64 bytes past it in 51,844 and exactly at it in 27. `PaletteSize` at `+0x18` is checked against 1024 rather than trusted. Channel order is `B,G,R,A`, locked on images that can say so — a greyscale ramp cannot — so a golden test asserts the `AEM_CLARION` wordmark decodes to 7,598 red pixels and no blue. The 12 stragglers are not a format: their embedded header fails its own hash self-check — they decompress fine and are refused three checks later — and they are 6 each in `IMPREZA` and `LANCER`, whose `VINYLS.BIN` are ~50 KB stubs of six descriptors where every other car's is 14 MB of 1,786. (The `compression` bullet above also says 54,873, of *blobs*; the two counts were measured separately and are not asserted here to be the same population.) See its module docs for the byte-level table.

**A pack is no longer small, and that is a caller's problem.** A car's `TEXTURES.BIN` is 73 images and 8.7 MB; a `VINYLS.BIN` is 1,786 images of 512² and **1.87 GB** (measured: 1.8 GB peak, 2.16 s). `Tpk::parse` decodes the lot without asking, so it is for a pack you have measured; `directory` + `decode_one` is for one you have not. The `ug2` CLI is unaffected — it only ever resolves `<car>/TEXTURES.BIN` — and PryHUB holds a pack to `doc::DECODE_BUDGET`. `texture::write` puts one back: `blob_of` hands out the decompressed blob, `replace_blob` recompresses it and writes it **in place**, updating only the descriptor's compressed size. In place because a TPK cannot simply be reassembled — its descriptors point at blobs by absolute file offset, and measured over 30 real packs the blobs are neither in descriptor order (1 of 30) nor contiguous (1 of 30). `replace_blob` recompresses with the codec the blob **arrived in**, because a slot sized by HUFF cannot be refilled by JDLZ: 1,451 of 2,123 fit (1,393/1,539 JDLZ, **59/584 HUFF**, where the HUFF share was 10 before there was a HUFF encoder). Install-wide the encoder re-writes all 52,389 real HUFF blobs byte-exact at **1.028×** EA's size against JDLZ's 1.242×, and the run escape is what carries that — with runs off, order-0 Huffman is 6.37× EA. **`relocate` is the general answer and now exists**: it lays every blob out afresh and rewrites every descriptor, so a replacement's size stops being a question. It rests on four things measured over the install's 77 chunked packs first — every blob lives in one leaf chunk (`0x33320002`, 54,875 of 54,875), that chunk is the **last leaf** (77 of 77), nothing outside the descriptor table points at a blob (every candidate hit was a misaligned straddle), and the bytes between blobs are junk rather than a pattern. Relocating all 77 with nothing replaced gives 54,875 byte-identical blobs for 0.36% more file — and **the game itself has read one**: a 240SX pack relocated with one texture repainted, so all 73 blobs moved, rendered with the edit visible and the other 72 correct. A decoder reading back what this crate wrote only shows the two agree; the game is what shows they are both right. `PEUGOT/TEXTURES.BIN` is the 78th and is refused: raw blocks with no wrapping chunk, the tolerant walk reading the first block's `JDLZ` magic as a chunk id. It was taken for "what a texture compiler does" until four packs made on purpose with that tooling (240SX, GOLF, RX7, SUPRA through NFS-CarToolkit) all turned out to **have** the chunk and to relocate unchanged — a compiler's ordinary output is an ordinary pack, only tidier than EA's: descriptor order, contiguous, all JDLZ. So PEUGOT is a rarer variant rather than a second format — and it is a **third-party mod, downloaded rather than built here**, so what wrote it is unknown and no second sample can be produced to check against. That, not the difficulty of the shape, is why it stays refused. `NFSU2_TOOLPACKS` gates the golden test over the four, the same way `NFSU2_PROFILES` gates the saves.
7b. **`texture::encode`** — the other direction, and the half that was missing: RGBA8 back into the
    format a blob already holds. It rewrites **only the image**, so the blob returns the same length
    and the embedded header, the `DebugName`, the dimensions and the format tag are never
    re-derived — which is also why a replacement must keep the texture's own dimensions. The mip
    chain is read out of the file rather than assumed: `ImageSize` is its length, where it *stops* is
    not a constant (over 2,423 textures it ends at 8×8 in 1,491, at 16×8 in 817, and 80 have no mips
    at all), and levels are laid down by halving until they fill `ImageSize` **exactly** — which they
    do 2,423 times out of 2,423. Writing it is safe because of a measurement, not an argument: the
    obvious guess is that the image ends before `out_size − header_from_end`, and `ImageSize` runs
    past that in 2,113 of 2,323; what does hold is that image and palette end before the `DebugName`
    in 2,323 of 2,323, in 2,313 of them by exactly twelve bytes. Measured by round-tripping every
    texture through the file, `0x20` comes back identical 436 times of 436 and the palettised tags
    120 of 120 — the two that can — while DXT3 is identical 378 of 742 and averages 44.9 dB over the
    364 that changed, DXT1 47.6 dB over 940, which is what two endpoints per 4×4 block costs. That
    last column counts only the textures that came back *changed*; the first version of it averaged a
    placeholder 99 dB for the identical ones and read 72.5 dB for DXT3. The palettised median cut is capped by the
    tag (256/16/64) so a re-encode cannot become the first file in the game to index past it, and
    the DXT arms score their candidates with the *decoder's* own `palette_from` and
    `build_alpha_table` rather than a second copy. `texture::replace_image` is the whole
    replacement in one call — encode, try in place, relocate if it does not fit (1,975 of 2,243 fit)
    — so `ug2 replace` and PryHUB cannot disagree about when a pack has to move.

8. **`placement`** — what a solid's local matrix *means*: a placement to apply, or a pose already baked into the vertices (`should_place`). Format semantics, so every consumer (the app, the CLI exporter, the game's engine layer) decides it the same way.
9. **`parts`** — **pure policy**, and deterministic: `select_car` returns the chosen parts in **file order**. It used to return them in `HashMap` iteration order, which Rust randomises per process — two exports of one car came out byte-different and the game drew the same parts in a different order every launch.: which material group a name is (`group_of`), what its `KIT##`/`KITW##`/`STYLE##` token says, and which parts make up a configuration (`select_car`). Lives here so the `ug2` CLI, the app and the game (a separate repo) all select identically; the game re-exports it as `nfsu2::parts`.
10. **`inspect`** — a chunk's bytes read back as labelled fields, each with the offset it came from (`model`). What an inspector pane draws; it reads through `geometry::format` so a viewer cannot drift from the parser about what a file says.
11. **`validate`** — the checks a person would run by hand: stride, bbox, normals, index range, chunk bounds. Every rule records **what it examined**, so "no findings" is never confused with "nobody looked".
12. **`discover`** — the inverse of `inspect`: read an *undecoded* chunk through a `Schema` (header + stride + column kinds) a person typed. It carries no per-chunk knowledge, only the arithmetic that cracks layouts in this format: `leading_filler` (the `0x11` run that is not part of the records), `stride_candidates`/`ranked_candidates` (strides that divide exactly, scored by whether their *lanes* hold a consistent kind of value — a divisor of the true stride mixes fields between lanes and scores badly, a multiple ties, so the answer is the best-scoring **shortest** stride), `stride_for` (`size / n`), and `guess_columns`. A golden test asserts `propose()` re-derives a real car's stride-36 vertex layout from bytes alone.
13. **`hash`** — `bStringHash`, the function NFSU2 names its assets by: `h = h * 33 + byte` from `0xFFFFFFFF`. **Locked empirically, not from a spec**: over one install's 2,123 TPK (`DebugName`, hash) pairs it reproduces the hash for every name that fits the 23-character name field and fails only for names of exactly that width — i.e. only where the input is known to be truncated. Two uses: name → key, and *confirming a guess*, which is the only way a truncated name's tail comes back (`240SX_DOORLINE_WIDEBODY` and its `_MASK` twin arrive under one truncated name; the hash tells them apart).
14. **`diff`** — two files, chunk by chunk: `Same` / `Changed` (same size, different bytes, with the first differing offset) / `Resized` / `OnlyLeft` / `OnlyRight`, and a container is `Changed` exactly when something inside it is. Chunks are paired **by position among siblings of the same id** — this format's trees are ordered, and any cleverer pairing would silently re-order parts and invent differences. Not a byte diff: after one edit every later offset has shifted, so bytes would be a wall of noise.
15. **`export`** — parsed data back out as files other tools read: `obj` (OBJ + MTL text), `gltf` (a self-contained `.glb`, images embedded — behind the `png` feature), `material` (`MaterialPlan`: which `newmtl`/glTF material a run resolves to, and the textures that implies), `png_name`/`png_bytes`. Pure — it returns text and bytes and never touches the filesystem, so `ug2` and PryHUB write the same car from the same code. **glTF is the one place a frame is converted**: the format *defines* +Y up / −Z forward, so `gltf` rotates `(x,y,z) → (−y,z,−x)` (a rotation, not a mirror) and leaves UVs alone, since glTF's UV origin is DirectX's. OBJ keeps the file's own frame and flips V.
15b. **`geometry::write`** — `rebuild`, which is [`repack::rebuild`] plus the thing it cannot know.
    `GEOMETRY.BIN` has the same disease a TPK has: `0x80134001 → 0x00134004` is one 24-byte record
    per solid holding its **absolute file offset**, so growing any chunk strands it. Measured over
    the install's 18,230 records — lane 1 the solid's offset, lanes 2 and 3 its size + 8, lanes 4–5
    zero, 18,230/18,230 each — and the records are **not in file order** (a 240SX's first record
    points at 5,154,048 while its first solid is at 19,712), so a fix-up pairs them by their old
    offset rather than by position. Growing one vertex buffer through the plain repacker strands
    over a hundred of a 240SX's 609 records; through this one every record still names a solid, the
    car reparses with every part intact, and a no-op rebuild is byte-identical. This is the
    foundation the mesh write stands on. On top of it, `replace_mesh` writes one solid's vertices,
    indices and bounding box back: **all 609 meshes in a 240SX round-trip byte-identically**, and a
    part moved a metre comes back moved with its box following. Two things that had to be measured
    rather than assumed, and both bit: the bbox is not the AABB but min−0.01 / max+0.01 (23,292 of
    23,299 solids), and the index buffer is anchored at its **leading** `0x11` filler while the
    vertex buffer is anchored at its tail — writing indices from the tail reintroduced the exact
    "shard" bug `geometry::index`'s own comment warns about, on a decal with four bytes of trailing
    pad. **A different topology goes in too**: halving a part's triangles shrinks the file, the mesh
    header's counts follow, the submesh runs retile, every other part is untouched and all 609 vertex
    buffers keep their alignment. That last one is why the write is a *loop* — the padding in front
    of each buffer is a function of where it lands (vertex data on a 128-byte boundary, indices and
    runs on 16, 18,225/18,225 each), which is a property of the rebuilt file, so it is built,
    measured and built again until the three settle. A solid is named by its **header offset**, since
    24.8% of them share a name. What is still missing is an *importer* — reading an edited model back
    in — and the one thing that cannot be measured from here is what Blender actually writes.

16. **`repack`** — the write path: `rebuild(bytes, edits)` reassembles a chunk stream, recomputing container sizes and alignment padding, replacing named leaf payloads. **Measured, not assumed**: every chunk size and offset in a real install is a multiple of 4; `0x80134010` (SolidObject) starts on a **128**-byte boundary 18,230 times out of 18,230, padded with an `id == 0` chunk only when needed; a 4-byte alignment debt is unpayable (a header is 8) so the files pay 132; `0x80034020` aligns to 64. An `id == 0` chunk is *not* always padding — BUS carries one of 1,332 bytes with a non-zero byte inside — so a gap is only re-derived when it is exactly what the rule would produce, and copied otherwise. A golden test rebuilds 113 files byte-for-byte.
17. **`globalb::carparts`** — the `CarParts` tables in `GLOBAL/GLOBALB.BUN`: 12,167 parts, 4,636
    attributes and the game's **paint palette**, which is the only place a car's colour is written
    down (`GEOMETRY.BIN` has none — NFSU2 does not texture a body, it paints it). Locked the way this
    crate locks everything: `0x00034603` is a header of *counts*, and three of its four multiply out
    to their chunk's size exactly (4636 × 8, 75 × 4, 1580 × 36); the fourth is 12,167 × 14 against a
    170,340-byte chunk, two bytes of the usual alignment. `parse` **checks every count against its
    chunk** rather than trusting it, so a differently-built bundle is refused instead of mis-read.
    Two fields of the 14-byte part record are claimed and no more: `+8` **× 4** is a string offset
    (all 12,167 land on a string start, and the raw value tops out at 9,213 where the blob is 36,860
    bytes) and `+12` indexes the attribute blocks with `0xFFFF` for none (exactly 1,580 distinct
    values, max 1,579 — that chunk's count). The 36-byte block is *not* decoded, so what links a part
    to its attributes is still unknown and `CarPart::block` hands over the raw index. Attribute keys
    are `hash::string_hash` of their names, which is how six of the fifty-one were confirmed —
    **49 of the 51** a real install uses, in `carparts::KNOWN`. A name is a candidate until the file
    agrees, and the test re-derives the whole table rather than trusting it, so a wrong pair cannot
    survive the build. The candidate words came from the NFS modding community's own tools; nothing
    but the words was taken — a name is a fact about the game's data, and the arithmetic, the layout
    and the code are this crate's. The remaining two keys stay unnamed rather than guessed at, and
    what the names say settles the question the search kept asking: `TEXTURE_NAME`, `DISPRED`,
    `HOODHUE`, `TIRESAT`, `SPINNER_TEXTURE`, `NUMREMAPCOLOURS`, `EXCLUDE_UG1` — this container is
    *visual customisation*, and the performance numbers are not in it. `palette()` reads colours as three *adjacent* attributes rather
    than through a part, because that link is missing and this does not need it: 123 colours, no
    component over 255. `ug2 globalb --parts` shows the lot.
18. **`profile`** — the player profile (`AppData/Local/NFS Underground 2/<name>/<name>`, 54,966 B,
    magic `20CM`): which performance products a car has fitted, and the per-category totals they add
    up to. Locked by **experiment**, not by reading: eight purchases made one at a time, the file
    diffed after each — the first wrote 388 bytes, every one after it 11–13. A product owns a *flag
    byte* and products sharing a slot replace one another (nitrous L2 moved the flag `+0x0BFC →
    `+0x0BFD`; a differential swap moved `+0x0BF8 → +0x0BF9`, which was **predicted before the
    purchase**). The per-category `f32` is the **sum of what is fitted**, each product carrying its
    own weight — 0.33 a step for the gearbox, 1.0 for nitrous, 0.21 then 0.30 for the engine; a
    "0..1 fill" model was held for two rounds and the file disproved it. Only the three measured
    slots are read: widening the window to the neighbours looked contiguous and was a guess, and the
    real saves rejected it. **The displayed torque and power are not in the file** — 3.64 / 1.75
    appear nowhere at any scale, so they are computed at run time, which is why no torque table was
    ever found in any static asset: there is not one. The **applied vinyl** is one `u32` at
    `+0x0E28`, holding `bStringHash` of its *menu* name — the pack calls it `240SX_FLAGS_SPAIN` and
    the save stores the hash of `FLAGS_SPAIN`. Three vinyls applied in turn each moved exactly
    twelve bytes (the digest at `+0x14`, and that one word), the third value was written down
    **before** the game saved, and hashing all 1,773 of a car's menu names against the whole
    54,966-byte profile returns exactly that one offset. The 48 zero bytes after it are probably
    further layers and are not read, for the reason the tuning array is only three slots wide. `NFSU2_PROFILES` gates the golden test, the
    saves being somebody's rather than fixtures that can ship.
19. **`types`** — the engine-agnostic output contract (see below).

The top-level `decompress_file()` is one of the few functions that touch the filesystem; everything downstream is pure `&[u8]`.

### Two hard invariants

- **Panic-free parsing.** The crate is `#![forbid(unsafe_code)]`. Input is always untrusted: every read is bounds-checked and returns an `NfsError`; no parse path may panic, `unwrap`, or allocate from an unchecked size field. `tests/no_panic.rs` enforces this with proptest against arbitrary/adversarial bytes — **any new parser must uphold it.**
- **Empirically-locked formats.** Several NFSU2 sub-formats have no public byte-level spec. Their exact offsets/constants are locked *empirically* using `ug2 dump`/`ug2 probe` against a legally-owned install, never by assuming unconfirmed constants. When touching format code, document offsets the way `geometry/mod.rs` does (chunk-ID map + stride/field-index constants) and validate against a real car. The objective correctness check for vertex layouts is `NfsMeshPart::indices_in_range()` (a correct layout yields all in-range indices).

### Output contract (`types`)

Pure-data structs, no `glam`/`wgpu`. Geometry is **indexed** and transforms are stored **as-in-file** (row-major, original handedness). Expanding indices to a flat vertex list and any coordinate-system fixups are deliberately the **integration layer's** job, not the parser's — e.g. the game's `geom::remap()` converts NFSU2's Z-up frame to Gizmo's (and `ug2 export` deliberately does not, writing the file's own frame). `serde` derives on all output types are gated behind the optional `serde` feature.

## Conventions

- Code comments and Cargo.toml notes are frequently in **Turkish** — match the surrounding language when editing a file.
- **Part names are truncated** to a fixed-length field in `GEOMETRY.BIN`. Long names lose their tail: `..._HEADLIGHT_LEFT_LOD_A` arrives as `..._HEADLIGHT_LEFT_` (LOD letter gone, so two LODs share a name — disambiguate by triangle count) and `..._SIDE_MIRROR` as `..._MIRRO`/`..._MIRR`. Match shortened stems (`MIRR`), never assume the full word survives. A truncated name can be *recovered* rather than guessed: `gizmo_nfs::hash` hashes a candidate and the file's own hash says whether it is right. `NfsTexture::name_is_whole()` asks whether the stored name survived the cut, and `NfsTexture::is_mask()` answers "is this another texture's `_MASK` companion" **through** the truncation — one install hides 56 such masks across 29 cars, all fully opaque, and binding one as a diffuse map is a black panel. The game's skin matching (`car::skin`) filters on that proof rather than on how transparent an image happens to be. Cars also carry `STYLE00..STYLE14` purchasable part variants and `KIT01+` body kits alongside the default `BASE`/`KIT00`; render only the default set or variants overlap.
- Status: the toolkit's phases 1–4 are done (read · visualise · export · discover), and **phase 5 is done for textures**: a pack can be edited from the interface or from `ug2 replace`. It was left last because TPK stores **absolute** offsets — changing one texture meant writing a *compressor*, recomputing every later offset and keeping the alignment — and all three of those now exist (`jdlz`/`huff` encoders, `texture::relocate`, `repack::rebuild`), with `texture::encode` supplying the pixels. Writing a *mesh* back is still untouched. The TPK texture format is **fully decoded**: per-texture DXT1/3/5, uncompressed BGRA and the palettised `0x08`/`0x80`/`0x81`, over JDLZ **and** HUFF blobs — 54,873 of the install's 54,885 declared textures, the remaining 12 being descriptors whose embedded header fails its own hash check rather than a format nobody has read. What is left is the world (`STREAM*.BUN`, `L4RA.BUN`). See `crates/gizmo-nfs/README.md` for the per-format status table.
