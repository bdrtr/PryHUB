//! Reading a Wavefront OBJ back in — the inverse of [`crate::export::obj`].
//!
//! The crate could write a car out and not read one back, which is the difference between an
//! exporter and a round trip. This is the half that closes it.
//!
//! # What an OBJ is not
//!
//! It is not the file's vertex list. OBJ keeps three independent pools — positions, texture
//! coordinates, normals — and a face names one of each per corner, so **the same position appears in
//! several vertices** when it carries different UVs or normals across a seam, and a position with no
//! seam appears once no matter how many triangles use it. NFSU2's buffer is the other shape: one
//! record per (position, normal, colour, uv) and an index list into it. So a reader cannot map
//! `v` lines onto vertices; it has to build a vertex per *distinct corner* and index them.
//!
//! That is also why the vertex count coming back out of an editor is almost never the one that went
//! in, and why [`crate::geometry::replace_mesh`] had to learn to change topology before this module
//! was worth writing.
//!
//! # What is undone here, and what is not
//!
//! [`crate::export::obj`] flips V on the way out, because OBJ's texture origin is the opposite
//! corner from DirectX's; this flips it back. It does **not** undo the placement matrix that
//! exporter bakes into the positions — that is the solid's, not the file's, and a reader handed a
//! bare `.obj` has no way to know which solid it belongs to. Un-placing is the caller's step, and
//! [`crate::placement`] is where the rule lives.
//!
//! Nothing here decides which *solid* a mesh replaces either. 24.8% of the install's solids share
//! their name with another solid in the same file, so the `o` line is a hint and never an identity.

use crate::error::{NfsError, NfsResult};

/// One mesh read out of an OBJ: an `o`/`g` group, or the whole file when it names none.
#[derive(Debug, Clone, Default)]
pub struct ObjMesh {
    /// The `o` (or failing that `g`) name this came under. Empty when the file names nothing.
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    /// Empty when the file carries no `vn` for these faces.
    pub normals: Vec<[f32; 3]>,
    /// Empty when the file carries no `vt`. V is flipped back to the file's own convention.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex colour, from the `v x y z r g b` extension. Empty when the file carries none —
    /// which is the usual case, and not the same as "every vertex is white".
    pub colours: Vec<[u8; 4]>,
    /// Triangle-list indices into the arrays above.
    pub indices: Vec<u32>,
    /// The `usemtl` runs, in the order their triangles were emitted, tiling [`Self::indices`].
    pub runs: Vec<ObjRun>,
}

/// One `usemtl` group's slice of the index list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjRun {
    /// The material name the OBJ gave it.
    pub material: String,
    pub offset: usize,
    pub count: usize,
}

/// The largest OBJ this will read, in lines.
///
/// A guard rather than a limit anyone meets: a whole 240SX is 4,000 lines of `o` groups and about
/// 900,000 of data. Reading is linear, so the cap is only here because the input is a file somebody
/// chose and the crate does not allocate from an unchecked size.
const MAX_LINES: usize = 40_000_000;

/// Read every mesh in an OBJ.
///
/// Faces are grouped by `usemtl` **within** each object, and the triangles are emitted run by run,
/// so [`ObjMesh::runs`] tiles the index list end to end — which is the shape NFSU2's `0x00134B02`
/// table needs and the one it has in 23,300 of 23,300 real solids. A face list that alternates
/// between two materials therefore comes back as two runs, not as the file's interleaving.
///
/// Polygons are fan-triangulated. Blender writes quads unless asked not to, and a reader that
/// refused them would refuse the default export.
///
/// # Errors
/// Malformed numbers, a face index that names nothing, or a file longer than this will read.
pub fn read(text: &str) -> NfsResult<Vec<ObjMesh>> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colours: Vec<Option<[u8; 4]>> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();

    // Faces are collected per object and per material, then flattened at the end — the runs have to
    // be contiguous, and the file is under no obligation to have written them that way.
    let mut objects: Vec<Group> = Vec::new();
    let mut current = Group::default();
    let mut material = String::new();

    for (n, line) in text.lines().enumerate() {
        if n > MAX_LINES {
            return Err(NfsError::BufferSizeMismatch { detail: "OBJ is longer than this will read" });
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else { continue };
        match tag {
            "v" => {
                let f: Vec<f32> = floats(&mut parts)?;
                if f.len() < 3 {
                    return Err(NfsError::CorruptArchive { detail: "an OBJ `v` needs three numbers" });
                }
                positions.push([f[0], f[1], f[2]]);
                // `v x y z r g b` — the vertex-colour extension, floats in 0..1.
                colours.push(if f.len() >= 6 {
                    Some([channel(f[3]), channel(f[4]), channel(f[5]), 255])
                } else {
                    None
                });
            }
            "vt" => {
                let f = floats(&mut parts)?;
                if f.len() < 2 {
                    return Err(NfsError::CorruptArchive { detail: "an OBJ `vt` needs two numbers" });
                }
                // Undo the flip the exporter applies: OBJ's origin is the far corner from DirectX's.
                uvs.push([f[0], 1.0 - f[1]]);
            }
            "vn" => {
                let f = floats(&mut parts)?;
                if f.len() < 3 {
                    return Err(NfsError::CorruptArchive { detail: "an OBJ `vn` needs three numbers" });
                }
                normals.push([f[0], f[1], f[2]]);
            }
            "o" | "g" => {
                // A `g` inside an object is a sub-group; only start a new mesh when something has
                // actually been put in the current one, or an OBJ that writes `o` then `g` would
                // come back as an empty mesh and a full one.
                let name = parts.collect::<Vec<_>>().join(" ");
                if current.any() {
                    objects.push(std::mem::take(&mut current));
                }
                current.name = name;
            }
            "usemtl" => material = parts.collect::<Vec<_>>().join(" "),
            "f" => {
                let corners: Vec<Corner> = parts
                    .map(|c| Corner::parse(c, positions.len(), uvs.len(), normals.len()))
                    .collect::<NfsResult<_>>()?;
                if corners.len() < 3 {
                    return Err(NfsError::CorruptArchive { detail: "an OBJ face needs three corners" });
                }
                // Fan triangulation, which is what a convex quad wants and what Blender's own
                // exporter assumes when it writes one.
                let bucket = current.bucket(&material);
                for k in 1..corners.len() - 1 {
                    bucket.push(corners[0]);
                    bucket.push(corners[k]);
                    bucket.push(corners[k + 1]);
                }
            }
            _ => {}
        }
    }
    if current.any() {
        objects.push(current);
    }

    let pool = Pool { positions: &positions, colours: &colours, uvs: &uvs, normals: &normals };
    objects.into_iter().map(|g| g.build(&pool)).collect()
}

/// The shared pools a face's corners index into.
struct Pool<'a> {
    positions: &'a [[f32; 3]],
    colours: &'a [Option<[u8; 4]>],
    uvs: &'a [[f32; 2]],
    normals: &'a [[f32; 3]],
}

/// One corner of a face: indices into the three pools, already resolved to zero-based.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Corner {
    position: usize,
    uv: Option<usize>,
    normal: Option<usize>,
}

impl Corner {
    /// `v`, `v/vt`, `v//vn` or `v/vt/vn`, one-based, and negative means "counting back from here".
    fn parse(text: &str, positions: usize, uvs: usize, normals: usize) -> NfsResult<Self> {
        let mut fields = text.split('/');
        let resolve = |field: Option<&str>, len: usize| -> NfsResult<Option<usize>> {
            let Some(f) = field.map(str::trim).filter(|f| !f.is_empty()) else { return Ok(None) };
            let i: isize =
                f.parse().map_err(|_| NfsError::CorruptArchive { detail: "an OBJ index is not a number" })?;
            let zero = if i > 0 {
                (i - 1) as usize
            } else if i < 0 {
                // Relative to the end of the pool *as it stands at this line*, which is why the
                // pools are passed in rather than counted afterwards.
                len.checked_sub(i.unsigned_abs())
                    .ok_or(NfsError::CorruptArchive { detail: "an OBJ index reaches before the file" })?
            } else {
                return Err(NfsError::CorruptArchive { detail: "an OBJ index of zero" });
            };
            if zero >= len {
                return Err(NfsError::CorruptArchive { detail: "an OBJ index names nothing" });
            }
            Ok(Some(zero))
        };
        let position = resolve(fields.next(), positions)?
            .ok_or(NfsError::CorruptArchive { detail: "an OBJ face corner with no position" })?;
        Ok(Self { position, uv: resolve(fields.next(), uvs)?, normal: resolve(fields.next(), normals)? })
    }
}

/// One `o`/`g` group's faces, kept per material so the runs come out contiguous.
#[derive(Default)]
struct Group {
    name: String,
    /// Material name → its corners, in the order the materials were first seen.
    runs: Vec<(String, Vec<Corner>)>,
}

impl Group {
    fn any(&self) -> bool {
        self.runs.iter().any(|(_, c)| !c.is_empty())
    }

    fn bucket(&mut self, material: &str) -> &mut Vec<Corner> {
        if let Some(i) = self.runs.iter().position(|(m, _)| m == material) {
            return &mut self.runs[i].1;
        }
        self.runs.push((material.to_owned(), Vec::new()));
        let last = self.runs.len() - 1;
        &mut self.runs[last].1
    }

    /// Turn corners into a vertex buffer and an index list.
    fn build(self, pool: &Pool<'_>) -> NfsResult<ObjMesh> {
        let mut seen: std::collections::HashMap<Corner, u32> = std::collections::HashMap::new();
        let mut mesh = ObjMesh { name: self.name, ..ObjMesh::default() };
        let have_uvs = !pool.uvs.is_empty();
        let have_normals = !pool.normals.is_empty();
        let have_colours = pool.colours.iter().any(Option::is_some);

        for (material, corners) in self.runs {
            if corners.is_empty() {
                continue;
            }
            let offset = mesh.indices.len();
            for c in corners {
                let next = seen.len() as u32;
                let index = *seen.entry(c).or_insert(next);
                if index == next {
                    // A corner nobody has used yet becomes a vertex.
                    let p = *pool
                        .positions
                        .get(c.position)
                        .ok_or(NfsError::CorruptArchive { detail: "an OBJ face names no position" })?;
                    mesh.positions.push(p);
                    if have_normals {
                        mesh.normals.push(c.normal.and_then(|i| pool.normals.get(i).copied()).unwrap_or([0.0, 0.0, 1.0]));
                    }
                    if have_uvs {
                        mesh.uvs.push(c.uv.and_then(|i| pool.uvs.get(i).copied()).unwrap_or([0.0, 0.0]));
                    }
                    if have_colours {
                        mesh.colours.push(
                            pool.colours.get(c.position).copied().flatten().unwrap_or([255; 4]),
                        );
                    }
                }
                mesh.indices.push(index);
            }
            mesh.runs.push(ObjRun {
                material,
                offset,
                count: mesh.indices.len() - offset,
            });
        }
        Ok(mesh)
    }
}

/// A 0..1 float as a byte, clamped — an OBJ colour is nominally in that range and nothing enforces it.
fn channel(v: f32) -> u8 {
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Every remaining field of a line as `f32`.
fn floats<'a>(parts: &mut impl Iterator<Item = &'a str>) -> NfsResult<Vec<f32>> {
    parts
        .map(|f| f.parse().map_err(|_| NfsError::CorruptArchive { detail: "an OBJ number is not a number" }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_with_every_attribute() {
        let obj = "\
v 0 0 0
v 1 0 0
v 0 1 0
vt 0 0
vt 1 0
vt 0 1
vn 0 0 1
usemtl paint
f 1/1/1 2/2/1 3/3/1
";
        let meshes = read(obj).expect("read");
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.normals.len(), 3);
        // V comes back flipped to the file's own convention.
        assert_eq!(m.uvs[0], [0.0, 1.0]);
        assert_eq!(m.runs, vec![ObjRun { material: "paint".into(), offset: 0, count: 3 }]);
    }

    /// The whole reason a reader cannot map `v` lines onto vertices.
    #[test]
    fn a_position_used_with_two_uvs_becomes_two_vertices() {
        let obj = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 1 1 0
vt 0 0
vt 1 1
vn 0 0 1
f 1/1/1 2/1/1 3/1/1
f 1/2/1 3/2/1 4/2/1
";
        let m = &read(obj).expect("read")[0];
        // Four positions, six vertices. Positions 1 and 3 are used by both faces with *different*
        // UVs, so each becomes two; positions 2 and 4 are used once. That is the whole reason a
        // reader cannot map `v` lines onto vertices, and the reason the count coming back out of an
        // editor is not the count that went in.
        assert_eq!(m.positions.len(), 6, "a seam splits a position, a shared corner does not");
        assert_eq!(m.indices.len(), 6);
        assert_eq!(m.uvs.len(), m.positions.len());
        // And nothing is duplicated that need not be: the two faces share no corner, so all six
        // indices are distinct.
        let distinct: std::collections::BTreeSet<u32> = m.indices.iter().copied().collect();
        assert_eq!(distinct.len(), 6);
    }

    #[test]
    fn a_quad_is_fanned_into_two_triangles() {
        let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";
        let m = &read(obj).expect("read")[0];
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert!(m.uvs.is_empty(), "a file with no vt gives no uvs rather than made-up ones");
        assert!(m.normals.is_empty());
    }

    #[test]
    fn faces_are_grouped_by_material_even_when_the_file_interleaves_them() {
        let obj = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 1 1 0
usemtl a
f 1 2 3
usemtl b
f 2 3 4
usemtl a
f 1 3 4
";
        let m = &read(obj).expect("read")[0];
        // Two runs, contiguous, and `a` holds both of its triangles — which is what the `0x00134B02`
        // table needs and what the file did not write.
        assert_eq!(m.runs.len(), 2);
        assert_eq!(m.runs[0], ObjRun { material: "a".into(), offset: 0, count: 6 });
        assert_eq!(m.runs[1], ObjRun { material: "b".into(), offset: 6, count: 3 });
        assert_eq!(m.indices.len(), 9);
    }

    #[test]
    fn objects_come_back_separately() {
        let obj = "\
v 0 0 0
v 1 0 0
v 0 1 0
o front
f 1 2 3
o rear
f 3 2 1
";
        let meshes = read(obj).expect("read");
        assert_eq!(meshes.len(), 2);
        assert_eq!(meshes[0].name, "front");
        assert_eq!(meshes[1].name, "rear");
        // The pools are shared across objects, which is what OBJ does and what a per-object reader
        // would get wrong.
        assert_eq!(meshes[1].positions.len(), 3);
    }

    #[test]
    fn negative_indices_count_back_from_where_they_are() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n";
        let m = &read(obj).expect("read")[0];
        assert_eq!(m.indices, vec![0, 1, 2]);
    }

    #[test]
    fn vertex_colours_are_read_when_the_file_carries_them() {
        let obj = "v 0 0 0 1 0 0\nv 1 0 0 0 1 0\nv 0 1 0 0 0 1\nf 1 2 3\n";
        let m = &read(obj).expect("read")[0];
        assert_eq!(m.colours, vec![[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]);
    }

    #[test]
    fn a_file_with_no_colours_says_so_rather_than_inventing_white() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        assert!(read(obj).expect("read")[0].colours.is_empty());
    }

    #[test]
    fn nonsense_is_an_error_rather_than_a_panic() {
        assert!(read("f 1 2 3").is_err(), "a face with no vertices to name");
        assert!(read("v 0 0\nf 1 1 1").is_err(), "a short v");
        assert!(read("v 0 0 0\nf 1 2 9").is_err(), "an index past the pool");
        assert!(read("v 0 0 0\nf 0 0 0").is_err(), "index zero");
        assert!(read("v x y z").is_err());
        // An empty file is an empty answer, not an error.
        assert!(read("").expect("read").is_empty());
        assert!(read("# nothing but a comment").expect("read").is_empty());
    }
}
