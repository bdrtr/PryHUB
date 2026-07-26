//! `ug2 import` — put an edited model back into a `GEOMETRY.BIN`.
//!
//! The far end of `ug2 export`. It reads an OBJ, undoes the two things the exporter did to it — the
//! V flip and the part's placement — and writes the mesh into a solid.
//!
//! Which solid is the question the file cannot answer. 24.8% of the install's solids share their
//! name with another solid in the same file, so a name is a hint: it is accepted when it names
//! exactly one, and otherwise the candidates are listed by the thing that does tell them apart,
//! their header offset, which `--part 0x…` takes.

use crate::paths::{read, Result};
use gizmo_nfs::chunk::ChunkNode;
use gizmo_nfs::geometry::{Mesh, Run};
use gizmo_nfs::placement::{part_centroid, should_place, Unplace};
use std::path::Path;

/// Which way up the model file is.
///
/// Not a property of OBJ — the format has no opinion — but of whatever wrote the file. `ug2 export`
/// writes NFSU2's own frame (x = length, y = width, z = height, Z up); **Blender's exporter defaults
/// to `Forward −Z, Up Y`** and rotates the whole car on the way out. Measured on a real round trip:
/// a vertex this crate wrote as `(-1.48727, 0.69655, 0.69480)` came back from Blender as
/// `(-1.48727, 0.69480, -0.69655)`, i.e. `(x, y, z) → (x, z, −y)`, a −90° turn about X.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axes {
    /// The file's own frame — what `ug2 export` writes and what needs no undoing.
    File,
    /// Y up, −Z forward: Blender's default.
    YUp,
}

impl Axes {
    /// Turn a model-file vector back into the game's frame.
    fn to_file(self, v: [f32; 3]) -> [f32; 3] {
        match self {
            Self::File => v,
            // The inverse of `(x, y, z) → (x, z, −y)`.
            Self::YUp => [v[0], -v[2], v[1]],
        }
    }

    /// What the text says about itself.
    ///
    /// `ug2 export` puts its own name in the first line, so a file this tool wrote is recognised
    /// rather than assumed; anything else is taken to be Y-up, because the overwhelmingly common
    /// other writer is Blender with its defaults. Either way the command *says* which it used —
    /// a model silently rotated a quarter turn is the kind of wrong that looks like a bad import.
    fn detect(text: &str) -> Self {
        if text.lines().next().is_some_and(|l| l.contains("gizmo-nfs")) {
            Self::File
        } else {
            Self::YUp
        }
    }
}

pub fn run(
    file: &Path,
    obj: &Path,
    part: Option<&str>,
    out: &Path,
    force: bool,
    axes: Option<Axes>,
) -> Result<()> {
    if !force && crate::replace::same_file(out, file) {
        return Err(format!(
            "{} is the file being read; pass --force to overwrite it, or -o somewhere else",
            out.display()
        ));
    }
    let bytes = read(file)?;
    let parts = gizmo_nfs::parse_geometry(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    let tree = ChunkNode::parse(&bytes).map_err(|e| format!("{e}"))?;
    let solids = mesh_solids(&tree, &bytes);
    if solids.len() != parts.len() {
        return Err(format!(
            "{}: {} solids with meshes but {} parts — this file is not one this command models",
            file.display(),
            solids.len(),
            parts.len()
        ));
    }

    let text = std::fs::read_to_string(obj).map_err(|e| format!("{}: {e}", obj.display()))?;
    let axes = axes.unwrap_or_else(|| Axes::detect(&text));
    let meshes = gizmo_nfs::import::obj::read(&text).map_err(|e| format!("{}: {e}", obj.display()))?;
    if meshes.is_empty() {
        return Err(format!("{}: no meshes in it", obj.display()));
    }

    // Which mesh, and which solid it goes into.
    let wanted = part.map(str::to_ascii_uppercase);
    let mesh = match (&wanted, meshes.len()) {
        (Some(name), _) => meshes
            .iter()
            .find(|m| m.name.to_ascii_uppercase() == *name)
            .or_else(|| meshes.iter().find(|m| m.name.to_ascii_uppercase().contains(name.as_str())))
            .ok_or_else(|| format!("{}: no mesh called {name} in it", obj.display()))?,
        (None, 1) => &meshes[0],
        (None, n) => {
            return Err(format!(
                "{} holds {n} meshes — say which with --part, or export one at a time",
                obj.display()
            ))
        }
    };
    let (index, solid) = locate(&parts, &solids, part.unwrap_or(&mesh.name))?;
    let target = &parts[index];

    // Undo the placement the exporter baked in.
    let apply = should_place(&target.transform, &part_centroid(target));
    let un = Unplace::new(&target.transform, apply)
        .ok_or_else(|| format!("{}: its placement cannot be inverted", target.name))?;
    // The frame first, then the placement: the rotation is what the *file* is in, the placement is
    // what the *part* is in, and undoing them the other way round turns the part about the car's
    // origin instead of its own.
    let positions: Vec<[f32; 3]> =
        mesh.positions.iter().map(|p| un.point(axes.to_file(*p))).collect();
    let normals: Vec<[f32; 3]> = if mesh.normals.is_empty() {
        Vec::new()
    } else {
        mesh.normals.iter().map(|n| un.dir(axes.to_file(*n))).collect()
    };
    if normals.len() != positions.len() {
        return Err(format!("{}: the OBJ carries no normals", obj.display()));
    }
    if mesh.uvs.len() != positions.len() {
        return Err(format!("{}: the OBJ carries no texture coordinates", obj.display()));
    }

    // Colour: the model's if it has any, else the part's own, else opaque white. An OBJ almost never
    // carries it, and the vertex the file holds is shading somebody baked — inventing white would
    // flatten it and look like it had worked.
    let colours: Vec<[u8; 4]> = if mesh.colours.len() == positions.len() {
        mesh.colours.clone()
    } else {
        (0..positions.len()).map(|i| target.colours.get(i).copied().unwrap_or([255; 4])).collect()
    };
    let kept_colours = mesh.colours.len() == positions.len();

    let runs: Vec<Run> =
        mesh.runs.iter().map(|r| Run { offset: r.offset, count: r.count }).collect();
    let written = gizmo_nfs::geometry::replace_mesh(
        &bytes,
        solid,
        &Mesh {
            positions: &positions,
            normals: &normals,
            colours: &colours,
            uvs: &mesh.uvs,
            indices: &mesh.indices,
            runs: Some(&runs),
        },
    )
    .map_err(|e| format!("{e}"))?;

    // Read back what is about to be written, before writing it — the same rule `ug2 replace` keeps.
    let back = gizmo_nfs::parse_geometry(&written)
        .map_err(|e| format!("the rewritten car does not parse back: {e}"))?;
    if back.len() != parts.len() {
        return Err("the rewritten car lost a part".into());
    }

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(out, &written).map_err(|e| format!("{}: {e}", out.display()))?;

    outln!("{} → {}  (solid {solid:#x})", mesh.name, target.name);
    outln!(
        "  read as {}",
        match axes {
            Axes::File => "the file's own frame (Z up)",
            Axes::YUp => "Y up, -Z forward — Blender's default, rotated back",
        }
    );
    outln!(
        "  {} vertices, {} triangles, {} material {}",
        positions.len(),
        mesh.indices.len() / 3,
        runs.len(),
        if runs.len() == 1 { "run" } else { "runs" }
    );
    let (dv, dt) = (
        positions.len() as i64 - target.positions.len() as i64,
        (mesh.indices.len() / 3) as i64 - (target.indices.len() / 3) as i64,
    );
    if dv != 0 || dt != 0 {
        outln!("  was {} vertices, {} triangles ({dv:+}, {dt:+})", target.positions.len(), target.indices.len() / 3);
    }
    if !kept_colours {
        outln!("  the OBJ carried no vertex colour; the part's own was kept");
    }
    outln!("  {} bytes → {} bytes → {}", bytes.len(), written.len(), out.display());
    Ok(())
}

/// Every solid that yields a part, in the order the parser produces them.
fn mesh_solids(nodes: &[ChunkNode], bytes: &[u8]) -> Vec<usize> {
    fn find(n: &ChunkNode, id: u32) -> Option<&ChunkNode> {
        if n.header.id == id {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    fn walk(nodes: &[ChunkNode], bytes: &[u8], out: &mut Vec<usize>) {
        for n in nodes {
            if n.header.id == 0x8013_4010 {
                if let (Some(mh), Some(vb)) = (find(n, 0x0013_4900), find(n, 0x0013_4b01)) {
                    let body = gizmo_nfs::geometry::skip_leading_filler(mh.data(bytes));
                    if let Some(b) = body.get(13 * 4..13 * 4 + 4) {
                        let verts = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                        let len = vb.data(bytes).len();
                        if verts > 0 && gizmo_nfs::geometry::standard_vertex_layout(verts, len) {
                            out.push(n.offset);
                        }
                    }
                }
            }
            walk(&n.children, bytes, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, bytes, &mut out);
    out
}

/// Which part and solid a name means. An offset is exact; a name has to be unambiguous.
fn locate(
    parts: &[gizmo_nfs::NfsMeshPart],
    solids: &[usize],
    want: &str,
) -> Result<(usize, usize)> {
    if let Some(hex) = want.strip_prefix("0x").or_else(|| want.strip_prefix("0X")) {
        let at = usize::from_str_radix(hex, 16).map_err(|_| format!("{want}: not an offset"))?;
        let i = solids
            .iter()
            .position(|s| *s == at)
            .ok_or_else(|| format!("{want}: no solid with a mesh at that offset"))?;
        return Ok((i, at));
    }
    let upper = want.to_ascii_uppercase();
    let hits: Vec<usize> = parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name.to_ascii_uppercase() == upper)
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        1 => Ok((hits[0], solids[hits[0]])),
        0 => Err(format!("{want}: no part of that name")),
        n => {
            // The case that makes a name useless as an identity, and it is common: 24.8% of the
            // install's solids share one.
            let mut msg = format!("{want} names {n} parts — say which by offset:\n");
            for i in hits.iter().take(12) {
                msg.push_str(&format!(
                    "  {:#x}  {} — {} vertices, {} triangles\n",
                    solids[*i],
                    parts[*i].name,
                    parts[*i].positions.len(),
                    parts[*i].indices.len() / 3
                ));
            }
            Err(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Axes;

    /// The rotation, from a real round trip rather than from the documentation.
    ///
    /// A vertex this crate wrote as `(-1.48727, 0.69655, 0.69480)` came back out of Blender 5.2 —
    /// exported with its defaults, `Forward -Z, Up Y` — as `(-1.48727, 0.69480, -0.69655)`. These
    /// are those two numbers.
    #[test]
    fn blenders_default_export_is_a_quarter_turn_about_x() {
        let ours = [-1.48727f32, 0.69655, 0.69480];
        let blender = [-1.48727f32, 0.69480, -0.69655];
        let back = Axes::YUp.to_file(blender);
        for k in 0..3 {
            assert!((back[k] - ours[k]).abs() < 1e-5, "{back:?} != {ours:?}");
        }
        // And a file already in the game's frame is left alone.
        assert_eq!(Axes::File.to_file(ours), ours);
    }

    /// Which frame a file is in, from what the file says about itself.
    #[test]
    fn our_own_obj_is_recognised_and_anything_else_is_taken_for_blenders() {
        assert_eq!(
            Axes::detect("# exported by gizmo-nfs — NFSU2 coordinates\nv 0 0 0\n"),
            Axes::File
        );
        assert_eq!(Axes::detect("# Blender 5.2.0 LTS\nv 0 0 0\n"), Axes::YUp);
        assert_eq!(Axes::detect(""), Axes::YUp);
    }
}
