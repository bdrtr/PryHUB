//! Writing indexed parts as Wavefront OBJ + MTL.
//!
//! Coordinates are NFSU2's own (x = length, y = width, z = height, Z-up) with each solid's
//! placement applied — the parser's contract is "as in file", and Z-up is what Blender reads
//! natively, so no axis fixup is invented here.
//!
//! One `o` group per part, one `usemtl` per material run, so a run that carries its own
//! texture stays separable in the importer.

use crate::placement::{part_centroid, place_dir, place_point, should_place};
use crate::types::NfsMeshPart;
use std::fmt::Write as _;

/// A material the OBJ references: either a texture map or a flat colour.
pub struct Material {
    /// `newmtl` name, referenced by `usemtl`.
    pub name: String,
    /// Relative path of the texture image, when the run resolved to one.
    pub map: Option<String>,
    /// Whether that image carries real transparency, and so should also drive the alpha
    /// channel (`map_d`). Binding an opaque image as an alpha map is harmless in principle but
    /// makes a whole panel vanish in an importer that reads the map's alpha loosely.
    pub map_is_cutout: bool,
    /// Diffuse colour used when there is no map.
    pub diffuse: [f32; 3],
}

/// Build the OBJ text for `parts`, naming each run's material via `material_for`.
///
/// `material_for(part, run_index)` returns the `newmtl` name to reference; a part with no
/// material list is emitted as a single run with index 0.
#[must_use]
pub fn write_obj(
    parts: &[&NfsMeshPart],
    mtl_file: &str,
    material_for: impl Fn(&NfsMeshPart, usize) -> String,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# exported by gizmo-nfs — NFSU2 coordinates (x=length, y=width, z=height)");
    let _ = writeln!(s, "mtllib {mtl_file}");
    // OBJ indices are 1-based and global to the file, so each part appends to the running count.
    let mut base = 1usize;
    for p in parts {
        let apply = should_place(&p.transform, &part_centroid(p));
        let _ = writeln!(s, "\no {}", p.name);
        for v in &p.positions {
            let g = place_point(&p.transform, *v, apply);
            let _ = writeln!(s, "v {:.5} {:.5} {:.5}", g[0], g[1], g[2]);
        }
        for n in &p.normals {
            let g = place_dir(&p.transform, *n, apply);
            let _ = writeln!(s, "vn {:.5} {:.5} {:.5}", g[0], g[1], g[2]);
        }
        for uv in &p.uvs {
            // OBJ's V axis points up, the game's down.
            let _ = writeln!(s, "vt {:.5} {:.5}", uv[0], 1.0 - uv[1]);
        }
        let has_n = !p.normals.is_empty();
        let has_uv = !p.uvs.is_empty();
        let face = |s: &mut String, tri: &[u32]| {
            let _ = write!(s, "f");
            for &i in tri {
                let v = base + i as usize;
                match (has_uv, has_n) {
                    (true, true) => {
                        let _ = write!(s, " {v}/{v}/{v}");
                    }
                    (true, false) => {
                        let _ = write!(s, " {v}/{v}");
                    }
                    (false, true) => {
                        let _ = write!(s, " {v}//{v}");
                    }
                    (false, false) => {
                        let _ = write!(s, " {v}");
                    }
                }
            }
            let _ = writeln!(s);
        };
        if p.materials.is_empty() {
            let _ = writeln!(s, "usemtl {}", material_for(p, 0));
            for tri in p.indices.chunks_exact(3) {
                face(&mut s, tri);
            }
        } else {
            for (run, m) in p.materials.iter().enumerate() {
                let Some(slice) = p.indices.get(m.index_offset..m.index_offset + m.index_count)
                else {
                    continue;
                };
                let _ = writeln!(s, "usemtl {}", material_for(p, run));
                for tri in slice.chunks_exact(3) {
                    face(&mut s, tri);
                }
            }
        }
        base += p.positions.len();
    }
    s
}

/// Build the MTL text for a set of materials.
#[must_use]
pub fn write_mtl(materials: &[Material]) -> String {
    let mut s = String::from("# exported by gizmo-nfs\n");
    for m in materials {
        let _ = writeln!(s, "\nnewmtl {}", m.name);
        let _ = writeln!(s, "Kd {:.3} {:.3} {:.3}", m.diffuse[0], m.diffuse[1], m.diffuse[2]);
        let _ = writeln!(s, "Ka 0.000 0.000 0.000");
        let _ = writeln!(s, "d 1.0");
        let _ = writeln!(s, "illum 2");
        if let Some(map) = &m.map {
            let _ = writeln!(s, "map_Kd {map}");
            if m.map_is_cutout {
                let _ = writeln!(s, "map_d {map}");
            }
        }
    }
    s
}

/// Apply a world solid's own placement to one of its vertices.
///
/// Row-vector convention, basis in rows 0-2 and translation in row 3, applied as-is: the layout is
/// byte-identical to a column-major OpenGL matrix, so transposing it puts the whole city on the
/// origin. Identity is checked rather than multiplied because 12,914 of the city's objects carry it.
#[must_use]
pub fn place_world_point(header: &crate::world::WorldSolidHeader, p: [f32; 3]) -> [f32; 3] {
    if !header.is_placed() {
        return p;
    }
    let m = &header.matrix;
    [
        p[0] * m[0][0] + p[1] * m[1][0] + p[2] * m[2][0] + m[3][0],
        p[0] * m[0][1] + p[1] * m[1][1] + p[2] * m[2][1] + m[3][1],
        p[0] * m[0][2] + p[1] * m[1][2] + p[2] * m[2][2] + m[3][2],
    ]
}

/// Build the OBJ text for a set of world solids, naming each run's material via `material_for`.
///
/// **`y_up` is about the reader, not the data.** The city is authored Z-up and so is Blender's
/// world, which is why this first shipped writing the file's own coordinates and calling the matter
/// closed. It is not: OBJ carries no axis convention, so every importer picks one, and the ones
/// that matter default to **Y-up** — Blender's arrives set to `Forward -Z, Up Y` and quietly turns
/// a Z-up model on its side. The file was right and the city stood up sideways anyway.
///
/// So both are offered and neither is hidden. `false` keeps the game's own frame, where a vertex in
/// the viewport is the same number `ug2` prints and the notes quote — which is worth something in a
/// tool whose whole job is locating things. `true` rotates `(x, y, z)` to `(x, z, -y)`, which is
/// what an importer left on its defaults expects, and the model comes in upright.
///
/// `material_for(mesh, run_index)` returns the `newmtl` name to reference; a solid with no runs is
/// emitted as one run with index 0 so geometry is never dropped for want of a material.
#[must_use]
pub fn write_world_obj(
    meshes: &[crate::world::WorldMesh],
    mtl_file: &str,
    y_up: bool,
    material_for: impl Fn(&crate::world::WorldMesh, usize) -> String,
) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# exported by gizmo-nfs — NFSU2 world coordinates, {}",
        if y_up { "rotated to Y-up for importers left on their defaults" } else { "Z-up as authored" }
    );
    let _ = writeln!(s, "mtllib {mtl_file}");
    let mut base = 1usize;
    for m in meshes {
        if m.positions.is_empty() || m.indices.is_empty() {
            continue;
        }
        let _ = writeln!(s, "\no {}", m.header.name);
        for v in &m.positions {
            let g = place_world_point(&m.header, *v);
            let g = if y_up { [g[0], g[2], -g[1]] } else { g };
            let _ = writeln!(s, "v {:.5} {:.5} {:.5}", g[0], g[1], g[2]);
        }
        for uv in &m.uvs {
            // OBJ's V axis runs the other way from the one the game samples with.
            let _ = writeln!(s, "vt {:.5} {:.5}", uv[0], 1.0 - uv[1]);
        }
        let uvs = m.uvs.len();
        let runs: Vec<(usize, usize, usize)> = if m.groups.is_empty() {
            vec![(0, 0, m.indices.len())]
        } else {
            m.groups
                .iter()
                .enumerate()
                .map(|(i, g)| (i, g.index_offset, g.index_count))
                .collect()
        };
        for (run, offset, count) in runs {
            let Some(slice) = m.indices.get(offset..offset + count) else { continue };
            let _ = writeln!(s, "usemtl {}", material_for(m, run));
            for t in slice.chunks_exact(3) {
                let f = |i: u32| {
                    let v = base + i as usize;
                    if (i as usize) < uvs { format!("{v}/{v}") } else { v.to_string() }
                };
                let _ = writeln!(s, "f {} {} {}", f(t[0]), f(t[1]), f(t[2]));
            }
        }
        base += m.positions.len();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NfsMaterialRange;

    fn part(name: &str) -> NfsMeshPart {
        NfsMeshPart {
            name: name.to_string(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![[0.25, 0.75]; 3],
            indices: vec![0, 1, 2],
            transform: crate::types::IDENTITY,
            ..NfsMeshPart::default()
        }
    }

    #[test]
    fn faces_are_one_based_and_run_on_after_the_first_part() {
        let (a, b) = (part("A"), part("B"));
        let obj = write_obj(&[&a, &b], "car.mtl", |_, _| "m".into());
        assert!(obj.contains("f 1/1/1 2/2/2 3/3/3"), "{obj}");
        // The second part's vertices continue the global numbering.
        assert!(obj.contains("f 4/4/4 5/5/5 6/6/6"), "{obj}");
    }

    #[test]
    fn each_material_run_gets_its_own_usemtl() {
        let mut p = part("A");
        p.indices = vec![0, 1, 2, 0, 1, 2];
        let run = |offset, count| NfsMaterialRange {
            index_offset: offset,
            index_count: count,
            ..NfsMaterialRange::default()
        };
        p.materials = vec![run(0, 3), run(3, 3)];
        let obj = write_obj(&[&p], "car.mtl", |_, run| format!("m{run}"));
        assert!(obj.contains("usemtl m0"));
        assert!(obj.contains("usemtl m1"));
    }

    #[test]
    fn v_axis_is_flipped_for_obj() {
        let obj = write_obj(&[&part("A")], "car.mtl", |_, _| "m".into());
        assert!(obj.contains("vt 0.25000 0.25000"), "{obj}");
    }

    #[test]
    fn mtl_references_its_map_when_there_is_one() {
        let mtl = write_mtl(&[
            Material {
                name: "tex".into(),
                map: Some("tex/1.png".into()),
                map_is_cutout: false,
                diffuse: [1.0; 3],
            },
            Material {
                name: "flat".into(),
                map: None,
                map_is_cutout: false,
                diffuse: [0.1, 0.2, 0.3],
            },
        ]);
        assert!(mtl.contains("newmtl tex") && mtl.contains("map_Kd tex/1.png"));
        assert!(mtl.contains("newmtl flat") && mtl.contains("Kd 0.100 0.200 0.300"));
        assert_eq!(mtl.matches("map_Kd").count(), 1);
    }
}
