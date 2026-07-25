//! Golden tests against a real, legally-owned NFSU2 install.
//!
//! These are **skipped unless `NFSU2_ROOT` is set** (to the game's install directory), so
//! CI and other machines stay asset-free. Run locally with, e.g.:
//!
//! ```bash
//! NFSU2_ROOT="/path/to/Need for Speed Underground 2" \
//!   cargo test -p gizmo-nfs --test golden_assets
//! ```

use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    std::env::var_os("NFSU2_ROOT").map(PathBuf::from)
}

/// The decisive JDLZ validation: `InGameCommon.lzc` (JDLZ-compressed) must decompress
/// byte-for-byte to `InGameCommon.bun` (the same bundle, uncompressed) shipped alongside it.
#[test]
fn jdlz_matches_ingamecommon_bun() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden JDLZ test");
        return;
    };
    let lzc = std::fs::read(root.join("GLOBAL/InGameCommon.lzc")).expect("read InGameCommon.lzc");
    let bun = std::fs::read(root.join("GLOBAL/InGameCommon.bun")).expect("read InGameCommon.bun");

    let out = gizmo_nfs::compression::jdlz::decompress(&lzc).expect("jdlz decompress");
    assert_eq!(out.len(), bun.len(), "decompressed length {} != bun length {}", out.len(), bun.len());
    if let Some(i) = out.iter().zip(bun.iter()).position(|(a, b)| a != b) {
        let lo = i.saturating_sub(4);
        panic!(
            "first mismatch at byte {i}: got {:02X?} want {:02X?}",
            out.get(lo..i + 4).unwrap_or(&[]),
            bun.get(lo..i + 4).unwrap_or(&[]),
        );
    }
}

/// A real car's `GEOMETRY.BIN` must parse as a chunk tree without error, with a top-level
/// `0x80134000` "solid list" container.
#[test]
fn geometry_bin_parses_as_chunk_tree() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping geometry parse test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/GEOMETRY.BIN")).expect("read GEOMETRY.BIN");
    let tree = gizmo_nfs::chunk::ChunkNode::parse(&bytes).expect("parse chunk tree");
    assert!(!tree.is_empty(), "expected at least one top-level chunk");
    assert_eq!(tree[0].header.id, 0x8013_4000, "expected solid-list container at top");
}

/// A real car's `TEXTURES.BIN` must parse with a top-level `0xB3300000` TPK container.
#[test]
fn textures_bin_is_a_tpk_container() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping textures parse test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/TEXTURES.BIN")).expect("read TEXTURES.BIN");
    let tree = gizmo_nfs::chunk::ChunkNode::parse(&bytes).expect("parse chunk tree");
    assert!(!tree.is_empty());
    assert_eq!(tree[0].header.id, 0xB330_0000, "expected TPK container at top");
}

/// The full geometry parser on a real car: many parts, the base body present with the
/// exact known counts, and every part's indices in range (the decisive layout check).
#[test]
fn geometry_parser_extracts_valid_parts() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping geometry parser test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/GEOMETRY.BIN")).expect("read GEOMETRY.BIN");
    let parts = gizmo_nfs::parse_geometry(&bytes).expect("parse geometry");
    assert!(parts.len() > 100, "240SX has hundreds of solids, got {}", parts.len());

    // Every part must have consistent, in-range, triangle-list geometry.
    for p in &parts {
        assert!(p.indices_in_range(), "part {} has out-of-range indices", p.name);
        assert_eq!(p.indices.len() % 3, 0, "part {} indices not a triangle list", p.name);
        assert_eq!(p.positions.len(), p.normals.len());
        assert_eq!(p.positions.len(), p.uvs.len());
    }

    // The base body, highest LOD, has the counts we locked during reverse-engineering.
    let base_a = parts.iter().find(|p| p.name == "240SX_BASE_A").expect("240SX_BASE_A present");
    assert_eq!(base_a.positions.len(), 483, "base_a vertex count");
    assert_eq!(base_a.triangle_count(), 496, "base_a triangle count");
    assert!(matches!(base_a.role, gizmo_nfs::PartRole::Body));
    assert!(matches!(base_a.lod, gizmo_nfs::LodLevel::A));
    // Normals are unit length.
    let n = base_a.normals[0];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    assert!((len - 1.0).abs() < 0.02, "normal not unit length: {len}");
}

/// The TPK parser on a real car: the known descriptor count, and every JDLZ-compressed
/// texture decoded to a correctly-sized RGBA8 image whose embedded header hash matches.
#[test]
fn tpk_parser_decodes_textures() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping TPK parser test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/TEXTURES.BIN")).expect("read TEXTURES.BIN");
    let tpk = gizmo_nfs::texture::Tpk::parse(&bytes).expect("parse TPK");

    // 240SX ships 73 textures; every one decodes now that both JDLZ and HUFF are supported.
    assert_eq!(tpk.entries.len(), 73, "descriptor count");
    assert_eq!(tpk.textures.len(), 73, "all 73 textures decode (JDLZ + HUFF)");
    assert_eq!(
        tpk.entries.iter().map(|e| e.header_from_end).collect::<std::collections::HashSet<_>>(),
        std::iter::once(0x100).collect(),
        "every descriptor's header_from_end is the constant 0x100"
    );

    let mut dxt1 = 0;
    for tex in tpk.textures.values() {
        // Dimensions are powers of two and the RGBA buffer is exactly W*H*4.
        assert!(tex.width.is_power_of_two() && tex.height.is_power_of_two(), "dims power of two");
        assert_eq!(tex.rgba.len(), tex.width as usize * tex.height as usize * 4, "tight RGBA8");
        assert_eq!(tex.format, gizmo_nfs::PixelFormat::Rgba8);
        if matches!(tex.source_format, gizmo_nfs::TexFormat::Dxt1) {
            dxt1 += 1;
        }
    }
    assert!(dxt1 >= 5, "expected several DXT1 textures, got {dxt1}");

    // The embedded DebugNames are recovered and carry the expected part-linked names.
    let names: Vec<&str> = tpk.textures.values().map(|t| t.name.as_str()).collect();
    assert!(names.iter().any(|n| n.starts_with("240SX_KIT00_HEADLIGHT")), "headlight texture named");
    assert!(names.iter().any(|n| n.starts_with("240SX_KIT00_BRAKELIGHT")), "brakelight texture named");
}

/// The discovery proposal, on a buffer whose layout is already known.
///
/// A car's `0x00134B01` vertex buffer is stride 36 (position, normal, colour, uv) behind a run of
/// `0x11` alignment filler. If [`gizmo_nfs::discover::propose`] cannot arrive at that from the
/// bytes alone, the screen built on it is guiding people to the wrong answer — which is worse than
/// leaving them to work it out.
#[test]
fn discovery_proposes_the_real_vertex_layout() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden discovery test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/GEOMETRY.BIN")).expect("read GEOMETRY.BIN");
    let tree = gizmo_nfs::chunk::ChunkNode::parse(&bytes).expect("chunk tree");
    let vb = tree.iter().find_map(|n| n.find(0x0013_4B01)).expect("a vertex buffer");
    let data = vb.data(&bytes);

    let schema = gizmo_nfs::discover::propose(data);
    let shape = gizmo_nfs::discover::shape(data.len(), &schema);
    assert_eq!(schema.stride, 36, "proposed {schema:?} for a stride-36 buffer");
    assert_eq!(shape.remainder, 0, "a correct stride leaves nothing over");
    // The header it found is the alignment filler, and nothing but.
    assert!(schema.header > 0 && data[..schema.header].iter().all(|b| *b == 0x11), "header is filler");

    // Position and normal are three floats each, and the guess must see them as floats.
    let floats = schema.columns.iter().filter(|k| **k == gizmo_nfs::discover::Kind::F32).count();
    assert!(floats >= 6, "only {floats} float lanes in {:?}", schema.columns);
}

/// The asset-name hash, against every name a real TPK carries.
///
/// The TPK's name field truncates at 23 characters, and a truncated name cannot hash to the value
/// computed from the full one — so the assertion is: every name that *fits* must hash correctly,
/// and every failure must be a name of exactly the field width. That is what locks the function
/// rather than merely agreeing with it.
#[test]
fn the_name_hash_reproduces_every_untruncated_tpk_name() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden hash test");
        return;
    };
    let (mut verified, mut examined) = (0usize, 0usize);
    for car in ["240SX", "RX7", "SUPRA", "TIBURON"] {
        let path = root.join("CARS").join(car).join("TEXTURES.BIN");
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let tpk = gizmo_nfs::Tpk::parse(&bytes).expect("tpk parses");
        for entry in tpk.textures.values() {
            let name = entry.name.trim_end_matches('\0');
            if name.is_empty() {
                continue;
            }
            examined += 1;
            if gizmo_nfs::hash::string_hash(name) == entry.hash.0 {
                verified += 1;
            } else {
                assert_eq!(
                    name.len(),
                    23,
                    "{car}: {name:?} hashes to {:#010x}, file says {:#010x} — and it is not a \
                     truncated name, so the hash function is wrong",
                    gizmo_nfs::hash::string_hash(name),
                    entry.hash.0
                );
            }
        }
    }
    // Most names in a TPK are truncated, so the verified share is a minority by construction;
    // these bounds only assert that the test really read four cars' worth of names.
    assert!(examined > 200, "only {examined} names examined — the test read nothing");
    assert!(verified > 40, "only {verified} of {examined} names verified");
}

/// Masks that the name field hid, found by hash.
///
/// A `_MASK` companion whose name was cut arrives under the name of the map it belongs to, and is
/// fully opaque — bound as a diffuse map it is a black panel, composited over paint it is a black
/// car. One install hides 56 of them across 29 cars, so this is not a corner case; the assertions
/// below are a floor, plus the two cases this project has actually been bitten by.
#[test]
fn truncation_hides_masks_that_the_hash_finds() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden mask test");
        return;
    };
    let mut hidden = 0usize;
    let mut named = 0usize;
    for dir in std::fs::read_dir(root.join("CARS")).expect("CARS/").flatten() {
        let Ok(bytes) = std::fs::read(dir.path().join("TEXTURES.BIN")) else { continue };
        let Ok(tpk) = gizmo_nfs::Tpk::parse(&bytes) else { continue };
        for tex in tpk.textures.values() {
            if tex.name.ends_with("_MASK") {
                named += 1;
            } else if tex.is_mask() {
                hidden += 1;
                // Every one of these is a full-coverage image; that is why mistaking one matters.
                let texels = (tex.rgba.len() / 4).max(1);
                let opaque = tex.rgba.chunks_exact(4).filter(|p| p[3] > 200).count();
                assert!(opaque * 10 >= texels * 9, "{}: a mask should be opaque", tex.name);
            }
        }
    }
    assert!(named > 20, "only {named} textures name themselves _MASK — read nothing?");
    assert!(hidden > 20, "only {hidden} hidden masks found — the hash is not seeing through the cut");

    // The two specific twins: on IMPREZAWRX the mask and its map arrive under one name, and on
    // 240SX the widebody doorline does the same.
    let impreza = std::fs::read(root.join("CARS/IMPREZAWRX/TEXTURES.BIN")).expect("IMPREZAWRX");
    let tpk = gizmo_nfs::Tpk::parse(&impreza).expect("tpk");
    let twins: Vec<_> =
        tpk.textures.values().filter(|t| t.name == "IMPREZAWRX_DOORLINE_KIT").collect();
    assert_eq!(twins.len(), 2, "the car ships the map and its mask under one truncated name");
    assert_eq!(
        twins.iter().filter(|t| t.is_mask()).count(),
        1,
        "exactly one of the twins is the mask"
    );
}

/// The repacker's foundation: rebuilding a file nobody edited must give the file back, byte for
/// byte, across a whole install.
///
/// This is the test that turns the layout rules in [`gizmo_nfs::repack`] from a reading of the
/// bytes into a claim that has been checked. Padding is *recomputed* from the alignment rules
/// rather than copied, so a single wrong boundary — or one solid that is not on 128 after all —
/// shows up here as a mismatch, on the first file that disagrees.
#[test]
fn every_file_rebuilds_byte_for_byte() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden repack test");
        return;
    };
    let edits = gizmo_nfs::repack::Edits::new();
    let (mut files, mut bytes_seen) = (0usize, 0usize);
    for car in std::fs::read_dir(root.join("CARS")).expect("CARS/").flatten() {
        for name in ["GEOMETRY.BIN", "TEXTURES.BIN"] {
            let path = car.path().join(name);
            let Ok(original) = std::fs::read(&path) else { continue };
            if gizmo_nfs::chunk::ChunkNode::parse(&original).is_err() {
                continue; // not a chunk stream; the repacker makes no claim about it
            }
            let rebuilt = gizmo_nfs::repack::rebuild(&original, &edits).expect("rebuild");
            files += 1;
            bytes_seen += original.len();
            assert_eq!(rebuilt.len(), original.len(), "{}: length changed", path.display());
            if let Some(at) = rebuilt.iter().zip(&original).position(|(a, b)| a != b) {
                panic!(
                    "{}: first difference at byte {at} (0x{at:X}): rebuilt {:02X?} vs original {:02X?}",
                    path.display(),
                    rebuilt.get(at..at + 8).unwrap_or(&[]),
                    original.get(at..at + 8).unwrap_or(&[]),
                );
            }
        }
    }
    assert!(files > 50, "only {files} files rebuilt — the test read nothing");
    eprintln!("rebuilt {files} files, {} MB, byte-exact", bytes_seen / (1024 * 1024));
}

/// The JDLZ compressor against the file the decompressor was locked with.
///
/// Byte-identity with EA's own stream is not the goal and would not be evidence of anything: two LZ
/// encoders that pick different matches both produce valid files. What is asked here is that a real
/// 1.6 MB bundle survives the round trip, and that the result is actually compressed — a "packer"
/// that emitted only literals would pass a round-trip test and be worthless. EA's own ratio on this
/// file is 30.1%, which is the number to be judged against.
#[test]
fn jdlz_compresses_a_real_bundle_and_reads_it_back() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden JDLZ compression test");
        return;
    };
    let bun = std::fs::read(root.join("GLOBAL/InGameCommon.bun")).expect("read InGameCommon.bun");
    let ea = std::fs::read(root.join("GLOBAL/InGameCommon.lzc")).expect("read InGameCommon.lzc");

    let packed = gizmo_nfs::compression::jdlz::compress(&bun).expect("compress");
    let back = gizmo_nfs::compression::jdlz::decompress(&packed).expect("decompress our own stream");
    assert_eq!(back.len(), bun.len(), "round-tripped length");
    assert!(back == bun, "a real bundle must survive the round trip byte for byte");

    let ours = packed.len() as f64 / bun.len() as f64;
    let theirs = ea.len() as f64 / bun.len() as f64;
    eprintln!(
        "jdlz: {} B → {} B ({:.1}%), EA's own {} B ({:.1}%)",
        bun.len(),
        packed.len(),
        ours * 100.0,
        ea.len(),
        theirs * 100.0
    );
    // On this file the encoder is slightly *better* than EA's. That is a regression lock, not a
    // boast: the in-place texture write below only works while the ratio holds.
    assert!(ours < theirs * 1.02, "ours {ours:.3} lost ground against EA's {theirs:.3}");

    // And the decompressor must accept a stream that mixes both token forms at scale — which this
    // one does, or the ratio above would be nowhere near.
    assert!(packed.len() > 16, "the stream has a body");
}

/// In-place texture replacement, on every texture of every real pack.
///
/// The mechanism cannot move anything, so the only question that decides whether it is useful is
/// empirical: does a blob recompressed by *our* encoder still fit the slot EA's encoder left for it?
/// This measures that over a whole install and asserts the round trip for the ones that fit — a
/// replaced texture must decode to exactly the pixels it decoded to before.
#[test]
fn a_texture_can_be_written_back_in_place() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden TPK write test");
        return;
    };
    let (mut fits, mut misses, mut packs, mut verified) = (0usize, 0usize, 0usize, 0usize);
    let mut worst = 0f64;
    for car in std::fs::read_dir(root.join("CARS")).expect("CARS/").flatten() {
        let Ok(file) = std::fs::read(car.path().join("TEXTURES.BIN")) else { continue };
        let Ok(tpk) = gizmo_nfs::Tpk::parse(&file) else { continue };
        if tpk.textures.is_empty() {
            continue;
        }
        packs += 1;
        let mut checked_this_pack = false;
        for (hash, before) in &tpk.textures {
            let Ok(blob) = gizmo_nfs::texture::blob_of(&file, *hash) else { continue };
            let entry = tpk.entry(*hash).expect("the entry we just read a blob for");
            let packed = gizmo_nfs::compression::jdlz::compress(&blob).expect("compress");
            worst = worst.max(packed.len() as f64 / entry.size as f64);
            if packed.len() > entry.size as usize {
                misses += 1;
                continue;
            }
            fits += 1;
            // The full verification is O(textures²) in this pack, so it runs once per pack; the
            // fit/miss counts above cover every texture in the install.
            if checked_this_pack {
                continue;
            }
            checked_this_pack = true;
            match gizmo_nfs::texture::replace_blob(&file, *hash, &blob) {
                Ok(out) => {
                    verified += 1;
                    assert_eq!(out.len(), file.len(), "an in-place write must not resize the file");
                    let after = gizmo_nfs::Tpk::parse(&out).expect("the rewritten pack parses");
                    let after_tex = after.texture(*hash).expect("the replaced texture is still there");
                    assert_eq!(after_tex.rgba, before.rgba, "the pixels came back unchanged");
                    assert_eq!(after_tex.width, before.width);
                    assert_eq!(after_tex.name, before.name);
                    // And every *other* texture is untouched, which is the point of not moving.
                    for (other, was) in &tpk.textures {
                        if other != hash {
                            assert_eq!(
                                after.texture(*other).map(|t| &t.rgba),
                                Some(&was.rgba),
                                "another texture changed"
                            );
                        }
                    }
                }
                Err(e) => panic!("a blob that fits must be writable: {e}"),
            }
        }
    }
    assert!(packs > 20, "only {packs} packs read");
    let total = fits + misses;
    eprintln!(
        "tpk in-place: {fits}/{total} textures fit their own slot ({:.0}%), verified in {verified} packs, worst {worst:.2}× the slot",
        fits as f64 / total as f64 * 100.0
    );
    assert!(verified > 20, "only {verified} packs verified");
    // The number that decides how useful this is today. It is a floor, not a target: a better
    // encoder raises it, and relocation removes the question.
    assert!(fits * 2 > total, "fewer than half the textures fit: {fits} of {total}");
}
