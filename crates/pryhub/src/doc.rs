//! An open file: its bytes, its chunk tree, and everything derived from them.
//!
//! One rule governs this module: **opening must never fail silently, and never fail wholly**. A
//! file whose geometry will not parse still has a chunk tree worth browsing and an error worth
//! reading — the design's whole premise is a tool that says "I am not sure about this" instead of
//! producing nothing. So the tree is walked tolerantly, the geometry pass is separate, and
//! whatever went wrong is kept as a [`Note`] for the log panel.

use gizmo_nfs::chunk::{ChunkKind, ChunkNode, WalkOptions};
use gizmo_nfs::validate::Severity;
use gizmo_nfs::NfsMeshPart;
use std::path::{Path, PathBuf};

/// How serious a note is. Mirrors the design's log-filter buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// One line in the log panel: what happened, and which chunk it happened to.
///
/// The line is not a sentence but a [`NoteKind`] plus its numbers, rendered when it is drawn. A
/// document is parsed on a worker thread that has no business knowing which language the window is
/// in — and a log written in the language that happened to be selected an hour ago would not switch
/// when the user does.
pub struct Note {
    pub level: Level,
    /// The chunk this is about, by its header offset — the same key selection uses, so clicking a
    /// log row can select the chunk it names.
    pub chunk: Option<usize>,
    /// The chunk id, pre-rendered for the log's middle column.
    pub chunk_id: String,
    pub kind: NoteKind,
}

/// What a log line is about. The data, not the words.
pub enum NoteKind {
    /// A compressed file was opened: raw size, decompressed size.
    Decompressed { from: usize, to: usize },
    /// It would not decompress, so the raw bytes are being read instead.
    DecompressFailed { error: String },
    ChunkTreeFailed { error: String },
    /// A solid that produced no part, and why. The reason is the parser's `SkipReason` rather than a
    /// sentence, for the same reason as everything else here.
    SolidDropped { name: String, reason: gizmo_nfs::SkipReason },
    GeometryFailed { error: String },
    /// The one-line summary of what opened.
    Opened { chunks: usize, parts: usize },
    /// A validation finding, already in the parser's own words.
    Finding { subject: String, message: String },
    /// An export that landed: what it wrote and where.
    Exported { summary: String, into: String },
    ExportFailed { error: String },
    /// Anything the program itself has to say — a panicking job, say. Not localised: it is a
    /// diagnostic that happens to be worth showing.
    Diagnostic(String),
}

impl NoteKind {
    /// The line, in the language the window is in.
    #[must_use]
    pub fn text(&self, t: &crate::i18n::Strings) -> String {
        match self {
            Self::Decompressed { from, to } => {
                format!("{from} B → {to} B {}", t.n_decompressed)
            }
            Self::DecompressFailed { error } => format!("{}: {error}", t.n_decompress_failed),
            Self::ChunkTreeFailed { error } => format!("{}: {error}", t.n_tree_failed),
            Self::SolidDropped { name, reason } => {
                format!("{name} → {}: {}", t.n_no_part, describe(*reason, t))
            }
            Self::GeometryFailed { error } => format!("{}: {error}", t.n_geometry_failed),
            Self::Opened { chunks, parts } => {
                format!("{chunks} {} · {parts} {}", t.st_chunks.of(*chunks), t.ex_parts.of(*parts))
            }
            Self::Finding { subject, message } => format!("{subject}: {message}"),
            Self::Exported { summary, into } => format!("{} — {summary} → {into}", t.exported),
            Self::ExportFailed { error } => format!("{}: {error}", t.export_failed),
            Self::Diagnostic(text) => text.clone(),
        }
    }
}

/// A flattened tree row: a node plus how deep it sits, so the tree panel can draw indentation
/// without walking the tree itself on every frame.
#[allow(dead_code)] // `size`/`data_offset` are what the inspector and hex panels will read next.
pub struct Row {
    /// Absolute offset of the chunk header — the selection key.
    pub offset: usize,
    /// The part name, for the solids that carry one. The design puts it right in the tree, and it
    /// is the difference between browsing 610 identical "SolidObject" rows and finding a panel.
    pub name: Option<String>,
    pub data_offset: usize,
    pub id: u32,
    pub size: u32,
    pub depth: usize,
    pub container: bool,
    /// Whether the node has children worth expanding.
    pub has_children: bool,
}

/// An open file and everything PryHUB knows about it.
pub struct Doc {
    pub path: PathBuf,
    /// The file as read from disk (decompressed if it carried a codec). Every borrowed slice in
    /// the app points into this, so it outlives the tree by construction.
    pub bytes: Vec<u8>,
    /// Whether the bytes on disk were compressed — the status bar's JDLZ indicator.
    pub codec: gizmo_nfs::compression::Codec,
    /// Top-level chunks; children hang off them.
    pub roots: Vec<ChunkNode>,
    /// Every node, depth-first, in the order the tree draws them.
    pub rows: Vec<Row>,
    /// The parts, when the file is a `GEOMETRY.BIN` that parses.
    #[allow(dead_code)]
    pub parts: Vec<NfsMeshPart>,
    /// What the checks concluded. Computed once at open; the tree, the log and the validation
    /// screen are three views of this one report.
    pub report: gizmo_nfs::validate::Report,
    /// Anything worth telling the user about *the open*. The document is immutable, so this is
    /// only what parsing found; what happens later in the session is the app's log.
    pub notes: Vec<Note>,
}

impl Doc {
    /// Read and analyse a file. Only a read error is fatal — a malformed *chunk stream* still
    /// yields whatever prefix parsed, with a note saying so.
    ///
    /// Four passes, each allowed to fail without taking the others down: decompress, walk the chunk
    /// tree, parse the geometry, run the checks. Every failure is a note rather than an error
    /// return, because a half-readable file is exactly what someone opens this tool for.
    pub fn open(path: &Path) -> Result<Self, String> {
        // Timed per phase at debug: this is the wait between asking for a file and seeing it, and
        // the only way to know which of the four passes owns it.
        let t = std::time::Instant::now();
        let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut notes = Vec::new();
        let codec = gizmo_nfs::compression::detect(&raw);
        let bytes = decompressed(raw, codec, &mut notes);
        let t_read = t.elapsed();

        let roots = chunk_tree(&bytes, &mut notes);
        let t_tree = t.elapsed();

        let mut rows = Vec::new();
        for r in &roots {
            flatten(r, &bytes, 0, &mut rows);
        }
        let t_rows = t.elapsed();

        let parts = geometry(&bytes, &mut notes);
        let t_geom = t.elapsed();

        // The checks run once, here — never per frame.
        let report = gizmo_nfs::validate::validate(&roots, &bytes);
        note_findings(&report, &mut notes);
        let t_valid = t.elapsed();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        log::debug!(
            target: "doc",
            "open {:.1} ms = read {:.1} + tree {:.1} + rows {:.1} + geometry {:.1} + validate {:.1}",
            ms(t_valid),
            ms(t_read),
            ms(t_tree - t_read),
            ms(t_rows - t_tree),
            ms(t_geom - t_rows),
            ms(t_valid - t_geom),
        );

        notes.push(Note {
            level: Level::Info,
            chunk: None,
            chunk_id: String::new(),
            kind: NoteKind::Opened { chunks: rows.len(), parts: parts.len() },
        });

        Ok(Self { path: path.to_path_buf(), bytes, codec, roots, rows, parts, report, notes })
    }

    /// The node whose header sits at `offset`.
    #[must_use]
    pub fn node_at(&self, offset: usize) -> Option<&ChunkNode> {
        fn find(nodes: &[ChunkNode], offset: usize) -> Option<&ChunkNode> {
            for n in nodes {
                if n.offset == offset {
                    return Some(n);
                }
                // Children are nested strictly inside the parent's span, so only descend when the
                // offset can actually be in there — on a 7 500-node tree that keeps this O(depth).
                if offset > n.offset && offset < n.data_offset + n.header.size as usize {
                    if let Some(found) = find(&n.children, offset) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&self.roots, offset)
    }

    /// The deepest chunk that owns byte `pos` — what the hex view highlights under the cursor.
    #[must_use]
    pub fn owner_of_byte(&self, pos: usize) -> Option<&ChunkNode> {
        fn walk<'a>(nodes: &'a [ChunkNode], pos: usize, best: &mut Option<&'a ChunkNode>) {
            for n in nodes {
                let end = n.data_offset + n.header.size as usize;
                if pos >= n.offset && pos < end {
                    *best = Some(n);
                    walk(&n.children, pos, best);
                }
            }
        }
        let mut best = None;
        walk(&self.roots, pos, &mut best);
        best
    }

    /// Decode this document's textures: the open file when it is itself a TPK, else the
    /// `TEXTURES.BIN` beside it — a car's geometry and its textures are two files.
    ///
    /// Pure, `&self`, and slow: a pack expanded to RGBA8. It is called from the worker thread
    /// ([`crate::jobs`]) rather than lazily from a panel, which is why a `Doc` needs no interior
    /// mutability and can be shared by an `Arc` instead of borrowed mutably.
    ///
    /// The second number is how many of the pack's textures were **not read**, which is a different
    /// answer from the ones that could not be decoded — see [`DECODE_BUDGET`].
    #[must_use]
    pub fn decode_textures(&self) -> Option<(gizmo_nfs::Tpk, usize)> {
        let own = decode_pack(&self.bytes).filter(|(t, _)| !t.entries.is_empty());
        own.or_else(|| {
            let beside = self.path.parent()?.join("TEXTURES.BIN");
            let bytes = std::fs::read(beside).ok()?;
            decode_pack(&bytes)
        })
    }

    /// The `0x80134010` solid a chunk belongs to, if any. A vertex buffer's stride can only be
    /// read together with the vertex count from its sibling mesh header, so the inspector needs
    /// the owning solid, not just the chunk.
    #[must_use]
    pub fn solid_of(&self, offset: usize) -> Option<&ChunkNode> {
        fn walk<'a>(nodes: &'a [ChunkNode], offset: usize, solid: Option<&'a ChunkNode>) -> Option<&'a ChunkNode> {
            for n in nodes {
                let end = n.data_offset + n.header.size as usize;
                if offset < n.offset || offset >= end {
                    continue;
                }
                let here = if n.header.id == gizmo_nfs::geometry::format::SOLID { Some(n) } else { solid };
                if n.offset == offset {
                    return here;
                }
                return walk(&n.children, offset, here);
            }
            solid
        }
        walk(&self.roots, offset, None)
    }

    /// The file's name, for the status bar and the window title.
    #[must_use]
    pub fn file_name(&self) -> String {
        self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// How many of the checks are unhappy, and how many are satisfied.
    ///
    /// Both numbers count **rules**: a rule that found something is a warning, a rule that read
    /// something and found nothing is a pass, and a rule that read *nothing* is neither — because
    /// "I did not look" is not a result. Two views draw this, the top bar's pill and the validation
    /// screen's chips, and the pill is a link to the screen; so it is counted once, here, rather
    /// than at each of them. It was counted at each of them, and they disagreed: the corner tallied
    /// rules while the screen tallied *findings*, so one bad normals rule with forty findings said
    /// `⚠ 1` in the corner and `40 warnings` on the screen it led to.
    #[must_use]
    pub fn health(&self) -> (usize, usize) {
        tally(self.report.results.iter().map(|r| (r.status(), r.examined)))
    }
}

/// The counting itself, over what each rule *concluded* rather than over the parser's own
/// `RuleResult` — which is `#[non_exhaustive]`, so only the parser can build one and [`Doc::health`]
/// would otherwise be untestable without a file on disk. The library is a published contract; it
/// should not grow a constructor so that this window can check its own arithmetic.
fn tally(rules: impl Iterator<Item = (Option<Severity>, usize)>) -> (usize, usize) {
    let mut warn = 0;
    let mut ok = 0;
    for (status, examined) in rules {
        match status {
            Some(_) => warn += 1,
            None if examined > 0 => ok += 1,
            None => {}
        }
    }
    (warn, ok)
}

/// The most decoded pixel data one pack is allowed to occupy, in bytes.
///
/// A pack used to be a car's, and a car's is 8.7 MB — so this held nothing back and did not exist.
/// Then the palettised formats started decoding, and `CARS/*/VINYLS.BIN` stopped being an empty
/// pack: 1,786 images, every one of them 512², which is **1.87 GB** of RGBA8 held resident behind an
/// `Arc` for as long as the file is open. Measured, that decode is 1.8 GB peak and 2.16 s against
/// the same car's 8.7 MB and 30 ms.
///
/// 256 MB is about thirty times the largest pack this tool was built to show and a seventh of the
/// largest it can now be handed. Nothing about a car changes — 8.7 MB is nowhere near the line — and
/// a vinyls pack gives as many of its images as fit instead of the none it gave last week or the
/// 1.87 GB it would give unchecked. At 512² that is 256 of the 1,786.
///
/// It is a **ceiling, not a policy**: the tool should decode what the sheet is actually showing, and
/// cannot yet, because a `TpkEntry` carries no name or dimensions — those live inside the compressed
/// blob — and the sheet sorts by name, so a lazily-filled grid would reshuffle as tiles landed.
pub const DECODE_BUDGET: usize = 256 * 1024 * 1024;

/// Decode a whole pack, spreading the textures over threads, up to [`DECODE_BUDGET`].
///
/// Every texture is an independent blob, and decoding one is a decompress plus a DXT pass — 20 ms
/// for a car on one thread, and the parser deliberately spawns none of its own. So the decision is
/// taken here, where it belongs: `directory` once, then the entries split between workers, then
/// `from_decoded`. Capped at eight because there is nothing to gain from more threads than the
/// memory bus.
///
/// Returns the pack and **how many entries were never kept**.
///
/// What comes back is the longest **prefix of file order whose decoded pixels fit the budget**, and
/// that is a property of the file alone — two runs on one file agree, on any number of threads.
/// Getting there takes two steps, because the threads cannot give it directly: `spent` counts
/// decodes that have *finished*, and up to `workers` claims are in flight past it, so the index at
/// which a worker first sees the budget spent depends on how the threads interleaved. That index
/// is only used to stop; it is not the answer. The answer is recomputed afterwards by walking the
/// decoded entries in index order and cutting where the running total would exceed the budget.
///
/// The recomputed cut is always reachable: workers only stop once `spent` has crossed the budget,
/// and everything counted in `spent` is below the stopping index, so what was decoded always covers
/// at least as far as the budget allows. A handful of decodes past the cut are thrown away, which is
/// the price of not serialising the workers to find out where it is.
fn decode_pack(bytes: &[u8]) -> Option<(gizmo_nfs::Tpk, usize)> {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    let entries = gizmo_nfs::Tpk::directory(bytes).ok()?;
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let next = AtomicUsize::new(0);
    let spent = AtomicUsize::new(0);
    let decoded = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (next, spent, decoded, entries) = (&next, &spent, &decoded, &entries);
            scope.spawn(move || {
                // Decode into a local batch and merge once: a lock per texture would serialise the
                // very thing being parallelised.
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Relaxed);
                    let Some(entry) = entries.get(i) else { break };
                    // Stop claiming once the budget is spent. Where it *falls* is decided below,
                    // from the entries rather than from whichever worker noticed first.
                    if spent.load(Relaxed) >= DECODE_BUDGET {
                        break;
                    }
                    if let Ok(tex) = gizmo_nfs::Tpk::decode_one(bytes, entry) {
                        spent.fetch_add(tex.rgba.len(), Relaxed);
                        mine.push((i, entry.hash, tex));
                    }
                }
                if let Ok(mut all) = decoded.lock() {
                    all.append(&mut mine);
                }
            });
        }
    });

    // Where the budget actually falls, decided by the file rather than by the threads: walk what
    // came back in index order and stop at the first texture that would take the total past the
    // line. `refused` only ever said "stop claiming"; it is deliberately not read here.
    let mut decoded = decoded.into_inner().unwrap_or_default();
    decoded.sort_unstable_by_key(|(i, _, _)| *i);
    let mut running = 0usize;
    let mut cut = entries.len();
    for (i, _, tex) in &decoded {
        running += tex.rgba.len();
        if running > DECODE_BUDGET {
            cut = *i;
            break;
        }
    }
    let unread = entries.len() - cut;
    let textures = decoded
        .into_iter()
        .filter(|(i, _, _)| *i < cut)
        .map(|(_, hash, tex)| (hash, tex))
        .collect();
    Some((gizmo_nfs::Tpk::from_decoded(entries, textures), unread))
}

/// Decompress when the magic says so, and say what happened either way. A codec that fails leaves
/// the raw bytes in place: a file that will not decompress is still worth looking at as bytes.
fn decompressed(
    raw: Vec<u8>,
    codec: gizmo_nfs::compression::Codec,
    notes: &mut Vec<Note>,
) -> Vec<u8> {
    if matches!(codec, gizmo_nfs::compression::Codec::None) {
        return raw;
    }
    match gizmo_nfs::compression::decompress(&raw) {
        Ok(b) => {
            notes.push(Note {
                level: Level::Info,
                chunk: None,
                chunk_id: format!("{codec:?}"),
                kind: NoteKind::Decompressed { from: raw.len(), to: b.len() },
            });
            b
        }
        Err(e) => {
            notes.push(Note {
                level: Level::Error,
                chunk: None,
                chunk_id: format!("{codec:?}"),
                kind: NoteKind::DecompressFailed { error: e.to_string() },
            });
            raw
        }
    }
}

/// Walk the chunk tree, tolerantly: a file whose tail is not a chunk stream (tool-compiled TPKs do
/// this) still shows the part that is.
fn chunk_tree(bytes: &[u8], notes: &mut Vec<Note>) -> Vec<ChunkNode> {
    let opts = WalkOptions { stop_on_overrun: true, ..WalkOptions::default() };
    match ChunkNode::parse_with(bytes, opts) {
        Ok(r) => r,
        Err(e) => {
            notes.push(Note {
                level: Level::Error,
                chunk: None,
                chunk_id: String::new(),
                kind: NoteKind::ChunkTreeFailed { error: e.to_string() },
            });
            Vec::new()
        }
    }
}

/// Parse the geometry, and name every solid that yielded no part.
///
/// Separate from the tree and allowed to fail: the tree is useful without it. A dropped solid used
/// to vanish without a word, which is how ~2,500 of them once went missing unnoticed.
fn geometry(bytes: &[u8], notes: &mut Vec<Note>) -> Vec<NfsMeshPart> {
    match gizmo_nfs::parse_geometry_reporting(bytes) {
        Ok((parts, dropped)) => {
            for s in dropped {
                notes.push(Note {
                    level: Level::Warn,
                    chunk: Some(s.offset),
                    chunk_id: format!("{:#010x}", gizmo_nfs::geometry::format::SOLID),
                    kind: NoteKind::SolidDropped {
                        name: if s.name.is_empty() { "(unnamed)".into() } else { s.name.clone() },
                        reason: s.reason,
                    },
                });
            }
            parts
        }
        Err(e) => {
            notes.push(Note {
                level: Level::Warn,
                chunk: None,
                chunk_id: String::new(),
                kind: NoteKind::GeometryFailed { error: e.to_string() },
            });
            Vec::new()
        }
    }
}

/// Turn the validation report into log lines.
fn note_findings(report: &gizmo_nfs::validate::Report, notes: &mut Vec<Note>) {
    for f in report.findings() {
        notes.push(Note {
            level: match f.severity {
                gizmo_nfs::validate::Severity::Error => Level::Error,
                gizmo_nfs::validate::Severity::Warn => Level::Warn,
                _ => Level::Info,
            },
            chunk: Some(f.chunk_offset),
            chunk_id: format!("{:#010x}", f.chunk_id),
            kind: NoteKind::Finding { subject: f.subject.to_string(), message: f.message.clone() },
        });
    }
}

/// Why a solid produced no part, in the interface's own words.
fn describe(reason: gizmo_nfs::SkipReason, t: &crate::i18n::Strings) -> String {
    match reason {
        gizmo_nfs::SkipReason::NotAMesh => t.skip_not_a_mesh.to_string(),
        gizmo_nfs::SkipReason::EmptyMesh => t.skip_empty.to_string(),
        gizmo_nfs::SkipReason::PackedVertexLayout { vert_count, vbuf_len } => format!(
            "{}: {vert_count} × {} > {vbuf_len} B",
            t.skip_packed,
            gizmo_nfs::geometry::VERTEX_STRIDE
        ),
        _ => t.skip_unknown.to_string(),
    }
}

/// Depth-first flatten, in draw order.
fn flatten(node: &ChunkNode, root: &[u8], depth: usize, out: &mut Vec<Row>) {
    out.push(Row {
        offset: node.offset,
        name: (node.header.id == gizmo_nfs::geometry::format::SOLID)
            .then(|| {
                node.find(gizmo_nfs::geometry::format::SOLID_HEADER)
                    .map(|h| gizmo_nfs::geometry::part_name(h.data(root)))
                    .filter(|n| !n.is_empty())
            })
            .flatten(),
        data_offset: node.data_offset,
        id: node.header.id,
        size: node.header.size,
        depth,
        container: node.kind() == ChunkKind::Container,
        has_children: !node.children.is_empty(),
    });
    for c in &node.children {
        flatten(c, root, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a chunk: 8-byte header (id LE, size LE) then payload.
    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&id.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// One rule's conclusion, as [`Doc::health`] hands it to [`tally`]: what it found, and how much
    /// it read to find it.
    fn rule(examined: usize, worst: Option<Severity>) -> (Option<Severity>, usize) {
        (worst, examined)
    }

    /// The pill in the corner is a tally of the marks the validation screen draws, so the two can
    /// never disagree about how the file is doing.
    #[test]
    fn a_rule_that_found_something_counts_as_a_warning() {
        assert_eq!(tally([rule(4, Some(Severity::Warn))].into_iter()), (1, 0));
        assert_eq!(tally([rule(4, Some(Severity::Error))].into_iter()), (1, 0));
    }

    #[test]
    fn a_rule_that_looked_and_found_nothing_counts_as_a_pass() {
        assert_eq!(tally([rule(9, None)].into_iter()), (0, 1));
    }

    /// The one case worth a test of its own: "I did not look" is not a pass, and counting it as one
    /// is exactly how a health indicator starts lying.
    #[test]
    fn a_rule_that_read_nothing_counts_as_neither() {
        assert_eq!(tally([rule(0, None)].into_iter()), (0, 0));
    }

    /// The two numbers partition the rules that looked — which is the property that lets the corner
    /// and the screen it links to be read against each other.
    #[test]
    fn every_rule_that_looked_lands_in_exactly_one_of_the_two() {
        let rules = [
            rule(4, Some(Severity::Warn)),
            rule(9, None),
            rule(0, None),
            rule(2, Some(Severity::Error)),
            rule(7, None),
        ];
        let (warn, ok) = tally(rules.into_iter());
        assert_eq!((warn, ok), (2, 2));
        assert_eq!(warn + ok, rules.iter().filter(|(_, examined)| *examined > 0).count());
    }

    /// `Doc::open` reads from disk, so a test needs a file. The name carries the test's own name
    /// because the suite runs in parallel and two tests sharing a path would delete each other's.
    fn doc_of(name: &str, bytes: Vec<u8>) -> Doc {
        let path = std::env::temp_dir().join(format!("pryhub-{name}.bin"));
        std::fs::write(&path, &bytes).unwrap();
        let doc = Doc::open(&path).unwrap();
        std::fs::remove_file(&path).ok();
        doc
    }

    #[test]
    fn rows_are_depth_first_and_carry_their_depth() {
        let leaf = chunk(0x0000_0002, &[1, 2, 3]);
        let inner = chunk(0x8000_0001, &leaf);
        let doc = doc_of("depth", inner);
        assert_eq!(doc.rows.len(), 2);
        assert_eq!((doc.rows[0].depth, doc.rows[0].container), (0, true));
        assert_eq!((doc.rows[1].depth, doc.rows[1].container), (1, false));
    }

    #[test]
    fn a_byte_maps_to_the_deepest_chunk_that_owns_it() {
        let leaf = chunk(0x0000_0002, &[1, 2, 3]);
        let inner = chunk(0x8000_0001, &leaf);
        let doc = doc_of("owner", inner);
        // A byte inside the leaf's payload belongs to the leaf, not to the container it sits in.
        let leaf_payload = doc.rows[1].data_offset;
        assert_eq!(doc.owner_of_byte(leaf_payload).map(|n| n.header.id), Some(0x0000_0002));
        // A byte in the container's own header belongs to the container.
        assert_eq!(doc.owner_of_byte(0).map(|n| n.header.id), Some(0x8000_0001));
    }

    #[test]
    fn a_broken_chunk_stream_still_opens_with_a_note() {
        // A leaf claiming far more payload than exists: the tolerant walk keeps the clean prefix,
        // and the file must still open — refusing to show anything is the failure mode this tool
        // exists to avoid.
        let mut bytes = chunk(0x0000_0011, &[1, 2, 3, 4]);
        bytes.extend_from_slice(&0x0000_0099u32.to_le_bytes());
        bytes.extend_from_slice(&0x7fff_ffffu32.to_le_bytes());
        bytes.extend_from_slice(&[0xDE, 0xAD]);
        let doc = doc_of("broken", bytes);
        assert_eq!(doc.rows.len(), 1, "the clean leaf before the bad region survives");
        assert!(doc.notes.iter().any(|n| n.level == Level::Warn || n.level == Level::Error));
    }

    #[test]
    fn node_lookup_finds_a_nested_chunk_by_its_offset() {
        let leaf = chunk(0x0000_0002, &[7; 8]);
        let inner = chunk(0x8000_0001, &leaf);
        let doc = doc_of("lookup", inner);
        let nested = doc.rows[1].offset;
        assert_eq!(doc.node_at(nested).map(|n| n.header.id), Some(0x0000_0002));
        assert_eq!(doc.node_at(nested + 1).map(|n| n.header.id), None, "offsets are exact");
    }
}
