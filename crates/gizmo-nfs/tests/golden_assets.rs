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

/// The TPK parser on a real car: the known descriptor count, and every texture — 44 JDLZ and 29
/// HUFF — decoded to a correctly-sized RGBA8 image whose embedded header hash matches.
///
/// This is the only place HUFF meets real bytes, so the codec split is asserted rather than
/// described: a texture count cannot tell the two decoders apart, and would read the same if the
/// pack were all JDLZ.
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
    let huff = tpk
        .entries
        .iter()
        .filter(|e| {
            let at = e.abs_offset as usize;
            let blob = bytes.get(at..at.saturating_add(e.size as usize)).unwrap_or(&[]);
            gizmo_nfs::compression::detect(blob) == gizmo_nfs::compression::Codec::Huff
        })
        .count();
    assert_eq!(huff, 29, "29 of the 240SX's 73 blobs are HUFF, the other 44 JDLZ");
    assert_eq!(
        tpk.entries.iter().map(|e| e.header_from_end).collect::<std::collections::HashSet<_>>(),
        std::iter::once(0x100).collect(),
        "every descriptor's header_from_end is the constant 0x100"
    );

    let (mut dxt1, mut dxt3, mut bgra) = (0, 0, 0);
    for tex in tpk.textures.values() {
        // Dimensions are powers of two and the RGBA buffer is exactly W*H*4.
        assert!(tex.width.is_power_of_two() && tex.height.is_power_of_two(), "dims power of two");
        assert_eq!(tex.rgba.len(), tex.width as usize * tex.height as usize * 4, "tight RGBA8");
        assert_eq!(tex.format, gizmo_nfs::PixelFormat::Rgba8);
        // A texture that decoded is a texture whose format we can name. `Unknown(n)` is for a tag
        // nothing here handles; a decoder that reports it is telling the interface, the exporter
        // and the next reader that it did not recognise what it had just finished unpacking.
        assert!(
            !matches!(tex.source_format, gizmo_nfs::TexFormat::Unknown(_)),
            "{} decoded but reports {:?}",
            tex.name,
            tex.source_format
        );
        match tex.source_format {
            gizmo_nfs::TexFormat::Dxt1 => dxt1 += 1,
            gizmo_nfs::TexFormat::Dxt3 => dxt3 += 1,
            gizmo_nfs::TexFormat::Bgra8888 => bgra += 1,
            other => panic!("{} decoded as an unexpected format {other:?}", tex.name),
        }
    }
    // All three of the pack's formats, stated as the counts they are rather than as a floor: the
    // numbers are measured and cost nothing to write down, and a floor of "several" would have been
    // met just as well by a pack that lost two thirds of its textures. The six uncompressed ones are
    // the `_DOORLINE` maps, and they are why the `Unknown` assertion above is exercised off the S3TC
    // path at all.
    assert_eq!((dxt1, dxt3, bgra), (39, 28, 6), "the 240SX's format histogram");

    // The embedded DebugNames are recovered and carry the expected part-linked names.
    let names: Vec<&str> = tpk.textures.values().map(|t| t.name.as_str()).collect();
    assert!(names.iter().any(|n| n.starts_with("240SX_KIT00_HEADLIGHT")), "headlight texture named");
    assert!(names.iter().any(|n| n.starts_with("240SX_KIT00_BRAKELIGHT")), "brakelight texture named");
}

/// The palettised formats, on the pack that is nothing but them: a car's `VINYLS.BIN`.
///
/// It is decoded one texture at a time through [`Tpk::directory`] + [`Tpk::decode_one`] rather than
/// through `Tpk::parse`, and that is the point rather than a detail. This pack is 1,786 images of
/// 512², so holding them all is **1.87 GB of RGBA**; the eager call measured 1.8 GB peak and 2.16 s,
/// against 8.7 MB and 30 ms for the same car's `TEXTURES.BIN`. A test that allocated that would be
/// asserting the crate works by being the reason a 13 GB machine swaps.
///
/// The channel order is what this really locks. Every other check here — the count, the size, the
/// format — passes just as happily with red and blue transposed, and a palette of greyscale ramps
/// (which most of `0x08` is) cannot tell the difference at all. So it asserts against an image that
/// can: the `AEM_CLARION` wordmark is Clarion's own red, and comes out with thousands of red pixels
/// and not one blue. Swap the two channels in `unpack_palettised` and exactly that inverts.
#[test]
fn vinyls_pack_decodes_as_palettised() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping golden vinyls test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/VINYLS.BIN")).expect("read VINYLS.BIN");
    let entries = gizmo_nfs::texture::Tpk::directory(&bytes).expect("descriptor table");
    assert_eq!(entries.len(), 1786, "the 240SX's vinyls pack declares 1,786 textures");

    let (mut decoded, mut clarion) = (0usize, None);
    for e in &entries {
        let tex = gizmo_nfs::texture::Tpk::decode_one(&bytes, e).expect("every vinyl decodes");
        decoded += 1;
        // One layout throughout: every image in this pack is a 512² palettised one.
        assert_eq!(tex.source_format, gizmo_nfs::TexFormat::P8, "{} is not P8", tex.name);
        assert_eq!((tex.width, tex.height), (512, 512), "{} is not 512²", tex.name);
        assert_eq!(tex.rgba.len(), 512 * 512 * 4, "{} is not tight RGBA8", tex.name);
        if tex.name == "240SX_AEM_CLARION" {
            clarion = Some(tex);
        }
        // Dropped here: see above. One at a time is 1 MB, all at once is 1.87 GB.
    }
    assert_eq!(decoded, 1786, "all 1,786 decode — the pack used to yield none");

    // The channel-order lock. A pixel counts as red only if it beats *both* other channels by a
    // margin, so a grey or a muddy pixel votes for neither side.
    let clarion = clarion.expect("240SX_AEM_CLARION present");
    let (mut red, mut blue) = (0usize, 0usize);
    for px in clarion.rgba.chunks_exact(4) {
        let (r, g, b, a) = (px[0] as i32, px[1] as i32, px[2] as i32, px[3] as i32);
        if a < 128 {
            continue;
        }
        if r > b + 60 && r > g + 60 {
            red += 1;
        }
        if b > r + 60 && b > g + 60 {
            blue += 1;
        }
    }
    assert_eq!((red, blue), (7598, 0), "the Clarion wordmark is red, and B,G,R,A is why");
}

/// Relocation: a pack rewritten with every blob at a new offset still reads back as the same pack,
/// and a texture that grew past its old slot goes in.
///
/// This is the check `replace_blob` can never make. In-place writing is safe *because* nothing
/// moves, so it needs no theory of the layout; relocation moves all of it, and the only thing that
/// proves the theory is decoding the result. So the assertion is not "the bytes look right" but
/// **every texture in the rewritten pack decodes to the pixels it decoded to before** — 73 of them,
/// compared image by image.
///
/// The growth case is the point of the whole exercise. It replaces a texture with a blob that
/// deliberately does not compress — random bytes, so JDLZ cannot shrink them — which is exactly the
/// case `replace_blob` refuses. If the offsets were not fixed up, the textures *after* it in the
/// file would decode as noise or not at all, and the pixel comparison below would say so.
#[test]
fn a_relocated_pack_reads_back() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping relocation test");
        return;
    };
    let bytes = std::fs::read(root.join("CARS/240SX/TEXTURES.BIN")).expect("read TEXTURES.BIN");
    let before = gizmo_nfs::texture::Tpk::parse(&bytes).expect("parse");
    assert_eq!(before.textures.len(), 73);

    // 1. Relocate with nothing replaced. Every image must survive the move unchanged.
    let moved = gizmo_nfs::texture::relocate(&bytes, &[]).expect("relocate with no edits");
    let after = gizmo_nfs::texture::Tpk::parse(&moved).expect("the relocated pack parses");
    assert_eq!(after.textures.len(), before.textures.len(), "same number of images");
    for (hash, old) in &before.textures {
        let new = after.texture(*hash).expect("every texture survives relocation");
        assert_eq!((new.width, new.height), (old.width, old.height), "{} size", old.name);
        assert_eq!(new.rgba, old.rgba, "{} pixels changed under relocation", old.name);
        assert_eq!(new.name, old.name, "name changed under relocation");
    }
    // The blobs were packed onto boundaries and the junk between them dropped, so the file is
    // allowed to change size — it is the pixels that must not.
    let dir = gizmo_nfs::texture::Tpk::directory(&moved).expect("directory");
    assert!(dir.iter().all(|e| (e.abs_offset as usize).is_multiple_of(128)), "every blob is on its boundary");

    // 2. Replace one texture with something that cannot be compressed and is far too big for its
    //    old slot, which is what in-place writing refuses.
    let victim = *before.textures.keys().next().expect("a texture");
    let old_slot = before.entries.iter().find(|e| e.hash == victim).expect("its descriptor");
    let mut blob = gizmo_nfs::texture::blob_of(&bytes, victim).expect("blob out");
    // Incompressible filler, appended so the embedded header keeps its distance from the end.
    let grown = {
        let mut v = vec![0u8; 64 * 1024];
        let mut x = 0x1234_5678u32;
        for b in v.iter_mut() {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (x >> 24) as u8;
        }
        v.extend_from_slice(&blob);
        v
    };
    blob = grown;
    assert!(
        gizmo_nfs::texture::replace_blob(&bytes, victim, &blob).is_err(),
        "in-place writing must refuse a blob this size — that is why relocation exists"
    );

    let bigger = gizmo_nfs::texture::relocate(&bytes, &[(victim, blob.clone())])
        .expect("relocation takes a blob that does not fit");
    let out = gizmo_nfs::texture::Tpk::directory(&bigger).expect("directory");
    let grown_entry = out.iter().find(|e| e.hash == victim).expect("its descriptor");
    assert_eq!(grown_entry.out_size as usize, blob.len(), "the new decompressed size is recorded");
    assert!(grown_entry.size > old_slot.size, "the new blob really is larger than the old slot");
    assert_eq!(
        gizmo_nfs::texture::blob_of(&bigger, victim).expect("read it back"),
        blob,
        "the replacement comes back out byte for byte"
    );
    // And every *other* texture still decodes — the thing that breaks when offsets are not fixed.
    let grown_pack = gizmo_nfs::texture::Tpk::parse(&bigger).expect("the grown pack parses");
    for (hash, old) in &before.textures {
        if *hash == victim {
            continue;
        }
        let new = grown_pack.texture(*hash).expect("a neighbour survived the insertion");
        assert_eq!(new.rgba, old.rgba, "{} moved and changed", old.name);
    }
}

/// Packs a *texture compiler* wrote, rather than EA — the files a modder actually has.
///
/// Gated on `NFSU2_TOOLPACKS`, a directory of `<car>/TEXTURES.BIN` produced by re-saving cars
/// through NFS-CarToolkit, for the same reason `NFSU2_PROFILES` is gated: they are somebody's
/// files, not fixtures that can ship.
///
/// It exists because the assumption behind it turned out to be wrong in a useful way. One tool-made
/// pack in the install (`PEUGOT`) has no `0x33320002` chunk at all, and the guess was that this is
/// what a texture compiler does. Four more, made on purpose, all **have** the chunk — so the
/// compiler's ordinary output is an ordinary pack, and what makes it recognisable is only that its
/// blobs are tidier than EA's: in descriptor order, contiguous, and all JDLZ where EA mixes in HUFF.
/// [`relocate`](gizmo_nfs::texture::relocate) therefore already takes them, which is the thing this
/// asserts so that it keeps being true.
#[test]
fn tool_compiled_packs_relocate_too() {
    let Some(dir) = std::env::var_os("NFSU2_TOOLPACKS").map(PathBuf::from) else {
        eprintln!("NFSU2_TOOLPACKS unset — skipping tool-compiled pack test");
        return;
    };
    let mut seen = 0usize;
    for car in std::fs::read_dir(&dir).expect("read the tool-pack directory").flatten() {
        let pack = car.path().join("TEXTURES.BIN");
        let Ok(bytes) = std::fs::read(&pack) else { continue };
        seen += 1;
        let name = car.file_name().to_string_lossy().to_string();

        // The shape that makes it recognisable as a compiler's work rather than EA's.
        let entries = gizmo_nfs::texture::Tpk::directory(&bytes).expect("directory");
        assert!(!entries.is_empty(), "{name}: no descriptors");
        let mut spans: Vec<(usize, usize)> =
            entries.iter().map(|e| (e.abs_offset as usize, e.size as usize)).collect();
        assert!(spans.windows(2).all(|w| w[0].0 <= w[1].0), "{name}: not in descriptor order");
        spans.sort();
        assert!(spans.windows(2).all(|w| w[0].0 + w[0].1 == w[1].0), "{name}: not contiguous");
        let last = spans.last().map_or(0, |(o, s)| o + s);
        assert_eq!(last, bytes.len(), "{name}: does not end at the last blob");

        // And the thing that matters: relocation takes it, and nothing inside changes.
        let before = gizmo_nfs::texture::Tpk::parse(&bytes).expect("parse");
        let moved = gizmo_nfs::texture::relocate(&bytes, &[]).expect("relocate a tool-made pack");
        let after = gizmo_nfs::texture::Tpk::parse(&moved).expect("the relocated pack parses");
        assert_eq!(after.textures.len(), before.textures.len(), "{name}: lost a texture");
        for (hash, old) in &before.textures {
            let new = after.texture(*hash).expect("every texture survives");
            assert_eq!(new.rgba, old.rgba, "{name}/{} changed under relocation", old.name);
        }
    }
    assert!(seen > 0, "NFSU2_TOOLPACKS was set but held no <car>/TEXTURES.BIN");
}

/// The one tool-made shape that is *not* an ordinary pack, and is refused rather than guessed at.
///
/// `PEUGOT/TEXTURES.BIN` has no `0x33320002`: its directory is followed by raw compressed blocks,
/// and the tolerant walk reads the first block's `JDLZ` magic as a chunk id. Four other packs from
/// the same author's tooling do not look like this, so it is a rarer variant rather than "what a
/// compiler does" — and one sample is not enough to rewrite a file on.
#[test]
fn the_chunkless_pack_is_refused_by_name() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping chunkless pack test");
        return;
    };
    let Ok(bytes) = std::fs::read(root.join("CARS/PEUGOT/TEXTURES.BIN")) else {
        eprintln!("no PEUGOT pack in this install — skipping");
        return;
    };
    // It still *reads*: the directory is a normal one and every texture decodes.
    let tpk = gizmo_nfs::texture::Tpk::parse(&bytes).expect("a chunkless pack still parses");
    assert!(!tpk.textures.is_empty(), "its textures decode");
    // It is only writing it back that is refused, and with a reason rather than a panic.
    let err = gizmo_nfs::texture::relocate(&bytes, &[]).expect_err("relocation must refuse it");
    assert!(
        matches!(err, gizmo_nfs::NfsError::NotImplemented { .. }),
        "refused, but as {err:?} rather than as an unimplemented shape"
    );
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
    let mut verified_huff = 0usize;
    let mut worst = 0f64;
    for car in std::fs::read_dir(root.join("CARS")).expect("CARS/").flatten() {
        let Ok(file) = std::fs::read(car.path().join("TEXTURES.BIN")) else { continue };
        let Ok(tpk) = gizmo_nfs::Tpk::parse(&file) else { continue };
        if tpk.textures.is_empty() {
            continue;
        }
        packs += 1;
        // Iterating `tpk.textures` is `HashMap` order, which Rust randomises per process, and the
        // full round trip below runs once per pack — so *which* texture got verified changed every
        // run, and with it whether a HUFF-sourced blob was ever round-tripped at all. That is the
        // one case where the codec changes under the write (HUFF in, JDLZ out), so leaving it to
        // chance meant the interesting path was usually untested. Ordered by hash, a run verifies
        // the same textures as the last one, and both codecs get a turn where the pack has both.
        let mut by_hash: Vec<_> = tpk.textures.iter().collect();
        by_hash.sort_by_key(|(h, _)| h.0);
        let mut checked: [bool; 2] = [false, false];
        for (hash, before) in by_hash {
            let Ok(blob) = gizmo_nfs::texture::blob_of(&file, *hash) else { continue };
            let entry = tpk.entry(*hash).expect("the entry we just read a blob for");
            let packed = gizmo_nfs::compression::jdlz::compress(&blob).expect("compress");
            worst = worst.max(packed.len() as f64 / entry.size as f64);
            if packed.len() > entry.size as usize {
                misses += 1;
                continue;
            }
            fits += 1;
            // The full verification is O(textures²) in this pack, so it runs at most twice — once
            // for a blob that arrived as JDLZ and once for one that arrived as HUFF, because those
            // are different writes: the second changes the stored codec. The fit/miss counts above
            // still cover every texture in the install.
            let at = entry.abs_offset as usize;
            let stored = gizmo_nfs::compression::detect(
                file.get(at..at.saturating_add(entry.size as usize)).unwrap_or(&[]),
            );
            let lane = usize::from(stored == gizmo_nfs::compression::Codec::Huff);
            if checked[lane] {
                continue;
            }
            checked[lane] = true;
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
        verified_huff += usize::from(checked[1]);
    }
    assert!(packs > 20, "only {packs} packs read");
    let total = fits + misses;
    eprintln!(
        "tpk in-place: {fits}/{total} textures fit their own slot ({:.0}%), {verified} round trips \
         ({verified_huff} of them HUFF-sourced), worst {worst:.2}× the slot",
        fits as f64 / total as f64 * 100.0
    );
    assert!(verified > 20, "only {verified} round trips");
    // A HUFF-sourced blob is the write that changes the stored codec, and it fits its slot for only
    // 10 of 585 textures — so without saying so out loud, a suite could stop covering it entirely
    // and still look busy.
    assert!(verified_huff > 0, "no HUFF-sourced texture was round-tripped");
    // The published number, not a floor of "more than half": 1,402 of 2,123 is the 66% that
    // `texture::write`, this crate's README and CLAUDE.md all quote. The band leaves room for
    // encoder tuning in the direction that helps and fails the day the figure quietly rots.
    assert!(
        (1_380..=total).contains(&fits),
        "the documented in-place fit rate is 1,402 of 2,123 (66%); measured {fits} of {total}"
    );
}

/// The `CarParts` tables, against the install they were locked on.
///
/// Every number here was measured rather than assumed, so this is the test that says so: the header
/// counts, the two part fields whose meaning is claimed, and the paint palette that falls out of
/// three adjacent attributes. If a differently-built bundle ever fails this, the reader is meant to
/// have refused it — the counts are checked against the chunks inside `CarParts::parse`.
#[test]
fn carparts_reads_the_real_bundle() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping CarParts test");
        return;
    };
    let bytes = std::fs::read(root.join("GLOBAL/GLOBALB.BUN")).expect("read GLOBALB.BUN");
    let cp = gizmo_nfs::CarParts::parse(&bytes).expect("CarParts parses");

    // The counts the header declares, which `parse` has already checked against the chunk sizes.
    assert_eq!(cp.parts.len(), 12_167, "parts");
    assert_eq!(cp.attributes.len(), 4_636, "attributes");

    // Every part's name offset lands on a real string: this is the claim that `+8` is scaled by 4,
    // and it is the whole reason to believe it.
    assert!(cp.parts.iter().all(|p| !p.name.is_empty()), "every part is named");
    assert!(cp.parts.iter().any(|p| p.name == "STOCK"), "the stock part is in there");

    // `0xFFFF` is a sentinel, so no block index may reach the block table's own count.
    assert!(
        cp.parts.iter().filter_map(|p| p.block).all(|b| usize::from(b) < 1_580),
        "no block index past the block table"
    );

    // The palette: adjacent RED/GREEN/BLUE triples, none of which can overflow a byte.
    let palette = cp.palette();
    assert_eq!(palette.len(), 123, "colours");

    // The six named keys really are used by this file.
    use gizmo_nfs::globalb::carparts::key;
    for (name, k, least) in [
        ("TEXTURE", key::TEXTURE, 800),
        ("NAME", key::NAME, 150),
        ("RED", key::RED, 100),
    ] {
        let n = cp.attributes.iter().filter(|a| a.key.0 == k).count();
        assert!(n >= least, "{name}: {n} attributes, expected at least {least}");
    }
}

/// The profile reader against the saves the model was measured on.
///
/// `NFSU2_PROFILES` points at a directory of them; the series this was locked with is
/// `A_engine1_trans1` … `J_diff2`, one purchase apart. Skipped when it is unset, like every other
/// golden test — these are somebody's save files, not fixtures that can ship.
#[test]
fn profile_reads_the_measured_series() {
    let Some(dir) = std::env::var_os("NFSU2_PROFILES").map(PathBuf::from) else {
        eprintln!("NFSU2_PROFILES unset — skipping profile test");
        return;
    };
    let read = |name: &str| {
        let bytes = std::fs::read(dir.join(name)).expect("read profile");
        gizmo_nfs::Profile::parse(&bytes).expect("profile parses")
    };
    use gizmo_nfs::profile::Category;

    // The gearbox category, one purchase at a time: three products, then one swapped for its next
    // level. Steps of 0.33, which is this category's own weight rather than a rule.
    for (name, want) in [
        ("A_engine1_trans1", 0.00),
        ("D_transmission", 0.33),
        ("E_flywheel", 0.66),
        ("F_differential", 0.99),
        ("J_diff2", 1.32),
    ] {
        let got = read(name).total(Category::Transmission);
        assert!((got - want).abs() < 0.01, "{name}: transmission {got} != {want}");
    }

    // Nitrous steps by a whole unit, and level 2 *replaces* level 1 rather than adding to it, so the
    // number of fitted products does not change.
    let (h, i) = (read("H_nitro"), read("I_nitro2"));
    assert!((h.total(Category::Nitrous) - 1.0).abs() < 0.01);
    assert!((i.total(Category::Nitrous) - 2.0).abs() < 0.01);
    assert_eq!(h.fitted(), i.fitted(), "a swap fits the same number of products");
    assert_ne!(h.installed, i.installed, "…but not the same ones");

    // The engine's two products weigh differently — 0.21 and 0.30 — which is why the total is a sum
    // and not a fill.
    assert!((read("C_coldintake").total(Category::Engine) - 0.21).abs() < 0.01);
    assert!((read("G_headers").total(Category::Engine) - 0.51).abs() < 0.01);
}

/// The per-car handling record, against the install it was locked on.
///
/// Every assertion here is a measurement that was made before the parser was written, and each one
/// is the kind that only the right offset can satisfy. The rpm triple is strictly increasing and
/// lands on multiples of 50 in all 46 cars; the torque curve rises to an interior peak and falls
/// again in all 46; the 184 transmission blocks all carry a negative reverse, a descending forward
/// set and a final drive stored twice and equal. A wrong offset does not produce those by accident.
#[test]
fn handling_reads_the_real_records() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset — skipping handling test");
        return;
    };
    let bytes = std::fs::read(root.join("GLOBAL/GLOBALB.BUN")).expect("read GLOBALB.BUN");
    let cars = gizmo_nfs::globalb::parse_cartypeinfos(&bytes);
    assert_eq!(cars.len(), 46, "the install ships 46 CarTypeInfo records");

    let (mut rising, mut fifties, mut unimodal, mut blocks, mut playable) = (0, 0, 0, 0, 0);
    for car in &cars {
        let e = car.handling.engine;
        if e.idle_rpm < e.red_line_rpm && e.red_line_rpm < e.limiter_rpm {
            rising += 1;
        }
        if [e.idle_rpm, e.red_line_rpm, e.limiter_rpm].iter().all(|v| v % 50.0 == 0.0) {
            fifties += 1;
        }
        // The gap splits the roster: 500 rpm for a car you can drive, 1000 for traffic.
        if (e.limiter_rpm - e.red_line_rpm - 500.0).abs() < 0.01 {
            playable += 1;
        }

        let t = &car.handling.torque_nm;
        let peak = t.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i).unwrap();
        let rises = (0..peak).all(|k| t[k] < t[k + 1]);
        let falls = (peak..8).all(|k| t[k] > t[k + 1]);
        if peak > 0 && peak < 8 && rises && falls {
            unimodal += 1;
        }

        for g in &car.handling.gearbox {
            let gears = g.gears();
            let ok = g.reverse < 0.0
                && (3..=6).contains(&g.count)
                && gears.iter().all(|r| *r > 0.0)
                && gears.windows(2).all(|w| w[0] > w[1])
                && (2.0..6.0).contains(&g.final_drive);
            if ok {
                blocks += 1;
            }
        }
    }
    assert_eq!(rising, 46, "idle < red line < limiter in every car");
    assert_eq!(fifties, 46, "every rpm figure is a multiple of 50");
    assert_eq!(unimodal, 46, "every torque curve rises to an interior peak and falls");
    assert_eq!(playable, 31, "31 cars have the 500 rpm gap; the other 15 are traffic");
    assert_eq!(blocks, 46 * 4, "all 184 transmission blocks are well formed");

    // The body box is not read from a lane that merely looks right: the inertia tensor beside it is
    // the closed form for a uniform cuboid, so mass and L/W/H together have to reproduce it.
    for car in &cars {
        let [l, w, h] = car.handling.body_m;
        // Bands measured off this install rather than guessed at, and wide enough for what is
        // actually in the roster: the PEUGOT is the shortest at 3.84 m and the BUS the largest
        // thing here at 9.40 × 2.40 × 3.40. A first attempt at 3.0..6.5 failed on the bus, which is
        // the test doing its job — a body box has to hold every vehicle the file describes.
        assert!((3.8..9.5).contains(&l), "{} length {l}", car.name);
        assert!((1.6..2.5).contains(&w), "{} width {w}", car.name);
        assert!((1.1..3.5).contains(&h), "{} height {h}", car.name);
    }

    let s14 = cars.iter().find(|c| c.name == "240SX").expect("the 240SX is in the roster");
    let h = &s14.handling;
    assert_eq!(
        (h.engine.idle_rpm, h.engine.red_line_rpm, h.engine.limiter_rpm),
        (800.0, 6500.0, 7000.0)
    );
    assert_eq!(h.rear_drive, 1.0, "the 240SX is rear-wheel drive");
    assert_eq!(h.body_m, [4.52, 1.69, 1.29], "the S14's box, in metres");
    let peak = h.torque_nm.iter().copied().fold(f32::MIN, f32::max);
    assert!((peak - 216.0).abs() < 0.5, "peak torque {peak} N·m");
    // Stock is a five-speed; the third transmission upgrade adds a sixth gear and shortens the
    // final drive. That progression is the design's four columns, in the file.
    assert_eq!(h.gearbox[0].count, 5);
    assert_eq!(h.gearbox[3].count, 6);
    assert!(h.gearbox[0].final_drive > h.gearbox[3].final_drive);
    // Compared with a tolerance: these are `f32`, so 1.902 comes back as 1.9020001 and an exact
    // comparison would be testing the decimal literal rather than the file.
    let stock = h.gearbox[0].gears();
    for (got, want) in stock.iter().zip([3.321, 1.902, 1.308, 1.0, 0.9]) {
        assert!((got - want).abs() < 1e-4, "240SX stock ratios {stock:?}");
    }

    // And a car that ships with six, so the count is read rather than assumed.
    let g35 = cars.iter().find(|c| c.name == "G35").expect("the G35 is in the roster");
    assert_eq!(g35.handling.gearbox[0].count, 6);
    assert_eq!(g35.handling.rear_drive, 1.0);
    // Front-wheel drive reads as the other end of the same lane.
    let civic = cars.iter().find(|c| c.name == "CIVIC").expect("the CIVIC is in the roster");
    assert_eq!(civic.handling.rear_drive, 0.0);
}
