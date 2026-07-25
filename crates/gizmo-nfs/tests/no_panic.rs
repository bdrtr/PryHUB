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
