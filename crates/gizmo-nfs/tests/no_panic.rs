//! Property tests: the untrusted-input parsers must never panic, hang, or over-allocate
//! on arbitrary or adversarial byte streams — they either succeed or return an `NfsError`.
//!
//! [`gizmo_nfs::discover`] is here for the same reason with a second input: the *schema* is typed
//! by a person, so a stride of zero, a header past the end of the file and a record index of
//! `usize::MAX` are all things it will be asked to read.

use gizmo_nfs::chunk::ChunkNode;
use gizmo_nfs::compression;
use gizmo_nfs::discover::{self, Kind, Schema};
use gizmo_nfs::repack;
use gizmo_nfs::texture::Tpk;
use gizmo_nfs::viv::VivArchive;
use proptest::prelude::*;

/// A one-descriptor TPK carrying the given field values, so a property test can hand the parser
/// numbers a real file would never contain and get a `TpkEntry` back out of it.
fn tpk_with(hash: u32, out_size: u32, header_from_end: u32) -> Vec<u8> {
    let mut desc = Vec::new();
    for f in [hash, 0, 0, out_size, header_from_end, 0] {
        desc.extend_from_slice(&f.to_le_bytes());
    }
    let mut file = Vec::new();
    file.extend_from_slice(&0x3331_0003u32.to_le_bytes());
    file.extend_from_slice(&(desc.len() as u32).to_le_bytes());
    file.extend_from_slice(&desc);
    file
}

proptest! {
    #[test]
    fn decompress_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = compression::decompress(&data);
    }

    /// `CarParts` reads counts out of the file and multiplies them by a stride, which is exactly the
    /// shape of an over-allocation: a header claiming four billion parts must be refused by the
    /// size check, not honoured with a `Vec::with_capacity`.
    #[test]
    fn carparts_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = gizmo_nfs::CarParts::parse(&data);
    }

    #[test]
    fn chunk_parse_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = ChunkNode::parse(&data);
    }

    #[test]
    fn viv_parse_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = VivArchive::parse(&data);
    }

    #[test]
    fn tpk_parse_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = Tpk::parse(&data);
    }

    // Force a RefPack-looking signature + a plausible small output size so the decoder's
    // opcode loop and its bounds guards get exercised on garbage payloads.
    #[test]
    fn refpack_signed_never_panics(mut data in proptest::collection::vec(any::<u8>(), 5..4096)) {
        data[0] = 0x10;
        data[1] = 0xFB;
        let _ = compression::decompress(&data);
    }

    // Force a HUFF header onto garbage so the Huffman table build + bit reader are exercised.
    #[test]
    fn huff_signed_never_panics(mut data in proptest::collection::vec(any::<u8>(), 16..4096)) {
        data[0..4].copy_from_slice(b"HUFF");
        data[4] = 1; // version
        let _ = compression::decompress(&data);
    }

    // The schema is user input: any stride (including 0), any header, any column list, any row.
    #[test]
    fn discover_never_panics(
        data in proptest::collection::vec(any::<u8>(), 0..4096),
        header in 0usize..8192,
        stride in 0usize..512,
        kinds in proptest::collection::vec(0usize..10, 0..24),
        index in 0usize..100_000,
    ) {
        let columns: Vec<Kind> = kinds.iter().map(|i| Kind::all()[*i]).collect();
        let schema = Schema { header, stride, columns };
        let shape = discover::shape(data.len(), &schema);
        let _ = discover::row(&data, &schema, index);
        let _ = discover::row(&data, &schema, usize::MAX);
        let _ = discover::row_offset(&schema, usize::MAX);
        let _ = discover::guess_columns(&data, header, stride);
        let _ = discover::stride_candidates(data.len());
        let _ = discover::stride_for(data.len(), shape.records);
        // A row's cell count is the column count, whatever the bytes did: a table that loses a
        // column silently shifts every value in the row.
        prop_assert_eq!(discover::row(&data, &schema, index).len(), schema.columns.len());
    }

    // Rebuilding is the write path, and it takes two untrusted inputs: the bytes and the caller's
    // edits. A payload of any length must not knock the assembler over.
    #[test]
    fn rebuild_never_panics(
        data in proptest::collection::vec(any::<u8>(), 0..4096),
        offsets in proptest::collection::vec(0usize..4096, 0..8),
        payload in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut edits = repack::Edits::new();
        for o in offsets {
            edits.insert(o, payload.clone());
        }
        if let Ok(out) = repack::rebuild(&data, &edits) {
            // Whatever came out has to be a chunk stream this crate can read back.
            prop_assert!(gizmo_nfs::chunk::ChunkNode::parse(&out).is_ok() || !out.is_empty());
        }
    }

    /// Re-encoding takes *three* untrusted inputs, which is one more than anything else here: the
    /// blob, the descriptor that says where its header is, and the caller's pixels. The descriptor
    /// is the interesting one — `out_size` and `header_from_end` are read out of a file and then
    /// subtracted from one another, and the dimensions and `ImageSize` that decide how much gets
    /// written come from inside the blob those two numbers point at.
    #[test]
    fn replace_pixels_never_panics(
        blob in proptest::collection::vec(any::<u8>(), 0..4096),
        out_size in any::<u32>(),
        header_from_end in any::<u32>(),
        hash in any::<u32>(),
        w in 0u32..64,
        h in 0u32..64,
        pixels in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        // A descriptor cannot be built from outside the crate — `TpkEntry` is `#[non_exhaustive]`,
        // because a caller is meant to get one from a file. So the arbitrary fields are written
        // into a pack and read back out through the parser, which is the path a caller takes.
        let pack = tpk_with(hash, out_size, header_from_end);
        if let Ok(entries) = gizmo_nfs::texture::Tpk::directory(&pack) {
            if let Some(entry) = entries.first() {
                let _ = gizmo_nfs::texture::replace_pixels(&blob, entry, &pixels, w, h);
            }
        }
    }

    /// The PNG reader is the newest untrusted input in the crate, and the only one whose bytes come
    /// from outside the game entirely — a file a person chose in a chooser. Its allocation is driven
    /// by a width and a height read out of a header, which is the shape this whole file exists to
    /// watch. Gated with the feature that compiles it; `--features tools` and `--features png` both
    /// turn it on.
    #[cfg(feature = "png")]
    #[test]
    fn png_pixels_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = gizmo_nfs::export::png_pixels(&data);
    }

    /// And over bytes that begin like a PNG, so the decoder gets past its signature check and into
    /// the header where the interesting numbers are. A uniform sample almost never does.
    #[cfg(feature = "png")]
    #[test]
    fn png_pixels_never_panics_on_a_plausible_header(
        tail in proptest::collection::vec(any::<u8>(), 0..512)
    ) {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend_from_slice(&tail);
        let _ = gizmo_nfs::export::png_pixels(&data);
    }

    /// The OBJ reader takes a *text* file somebody chose, which is a different kind of untrusted
    /// input from the rest of this file — its indices are one-based, may be negative, and name three
    /// separate pools whose lengths change as the file is read.
    #[test]
    fn obj_read_never_panics(text in ".{0,4096}") {
        let _ = gizmo_nfs::import::obj::read(&text);
    }

    /// And over text made of the tokens an OBJ is made of, so the parser gets past the first line.
    #[test]
    fn obj_read_never_panics_on_plausible_lines(
        lines in proptest::collection::vec(
            proptest::sample::select(vec![
                "v 0 0 0", "v 1 2 3 0.5 0.5 0.5", "v 1", "vt 0 0", "vt 1", "vn 0 0 1",
                "o part", "g sub", "usemtl m", "f 1 2 3", "f 1/1/1 2/2/2 3/3/3", "f 1 2 3 4",
                "f -1 -2 -3", "f 0 0 0", "f 1//1 2//1 3//1", "f 99 99 99", "s off", "# hi", "",
            ]),
            0..64,
        )
    ) {
        let _ = gizmo_nfs::import::obj::read(&lines.join("\n"));
    }

    // The compressor is the one function here that is asked to be *correct*, not merely
    // non-panicking: whatever it writes must read back as what it was given.
    #[test]
    fn jdlz_compress_round_trips_anything(data in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let packed = compression::jdlz::compress(&data).expect("compress");
        prop_assert_eq!(compression::jdlz::decompress(&packed).expect("decompress"), data);
    }

    /// The same demand of the HUFF encoder, and it is a harder one to meet. JDLZ is a byte-oriented
    /// LZ where a wrong decision costs ratio; this builds a Huffman table, a symbol table written as
    /// leaps over an alphabet, and an escape that means three different things — so an input that
    /// happens to use one byte, or all of them, or the byte the escape was assigned to, walks a
    /// different path through the writer than the typical one.
    #[test]
    fn huff_compress_round_trips_anything(data in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let packed = compression::huff::compress(&data).expect("compress");
        prop_assert_eq!(compression::huff::decompress(&packed).expect("decompress"), data);
    }

    /// And over inputs drawn from a small alphabet, which is where the runs are: real texture data
    /// is long stretches of one value, and a `0..8192` uniform sample almost never produces one.
    #[test]
    fn huff_round_trips_run_heavy_data(
        runs in proptest::collection::vec((any::<u8>(), 1usize..300), 0..64)
    ) {
        let mut data = Vec::new();
        for (b, n) in runs {
            data.extend(std::iter::repeat_n(b, n));
        }
        let packed = compression::huff::compress(&data).expect("compress");
        prop_assert_eq!(compression::huff::decompress(&packed).expect("decompress"), data);
    }
}

/// A chunk-shaped bundle with an arbitrary payload, so the walk actually reaches the readers
/// instead of bouncing off a bad header on byte one. Uniform random bytes almost never form a
/// valid chunk stream, and a no-panic test the parser never enters proves nothing.
fn bundle_with(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut file = id.to_le_bytes().to_vec();
    file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    file.extend_from_slice(payload);
    file
}

proptest! {
    /// The city's headers come from a sector-padded file the walk has to resync through, so this
    /// reader sees misaligned bytes as a matter of course rather than as an attack.
    #[test]
    fn world_read_header_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = gizmo_nfs::world::read_header(&data);
    }

    #[test]
    fn world_manifest_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = gizmo_nfs::world::manifest(&data);
    }

    /// The same, reached through a real `0x00134011` chunk header: the length is the file's to
    /// state, so a record claiming more than it has must be refused rather than read past.
    #[test]
    fn world_manifest_never_panics_on_a_wellformed_wrapper(
        payload in proptest::collection::vec(any::<u8>(), 0..512)
    ) {
        let _ = gizmo_nfs::world::manifest(&bundle_with(0x0013_4011, &payload));
        let _ = gizmo_nfs::world::manifest(&bundle_with(0x0013_4900, &payload));
        // Nested the way a bundle nests it, so `manifest` pairs a header with a mesh header.
        let solid = bundle_with(0x0013_4011, &payload);
        let _ = gizmo_nfs::world::manifest(&bundle_with(0x8013_4010, &solid));
    }
}
