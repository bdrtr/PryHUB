//! The 3D preview: one mesh, rendered offscreen and handed to egui as an image.
//!
//! Offscreen rather than a paint callback into egui's own pass, for one reason: that pass has no
//! depth attachment, and a solid drawn without a depth buffer shows its far side through its near
//! side. Rendering to our own texture costs one blit and buys correct occlusion.
//!
//! The vertices come straight from [`gizmo_nfs::NfsMeshPart`] with the file's own placement
//! applied ([`gizmo_nfs::placement`]) and its own frame kept (Z-up, 1 unit = 1 m), so what the
//! viewport shows is what the file says — not a re-interpretation of it.

use crate::gpu::math::M4;
use crate::theme;
use eframe::egui_wgpu::RenderState;
use gizmo_nfs::placement::{part_centroid, place_dir, place_point, should_place};
use gizmo_nfs::NfsMeshPart;
use wgpu::util::DeviceExt as _;

/// One vertex: position and normal in the file's frame, and the texture coordinate beside them.
///
/// The UV is taken as the file writes it. NFSU2's origin is DirectX's — top-left — which is also
/// wgpu's, so it is sampled straight; it is the OBJ exporter that has to flip V, not this.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

/// The uniform block the shader reads.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    clip_from_world: M4,
    /// `xyz` = the key light's direction, `w` unused.
    key_dir: [f32; 4],
}

/// One draw: a slice of the index buffer, and the texture it is drawn with.
///
/// The file already splits a part's indices this way (`NfsMaterialRange`), so the runs are read
/// rather than invented — the same table `MaterialPlan` reads to decide what an export writes.
struct Run {
    start: u32,
    count: u32,
    /// Index into [`Preview::materials`]; 0 is the white fallback.
    material: usize,
}

/// What is currently uploaded, so a redraw of the same selection costs nothing.
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// The draws, in material order.
    runs: Vec<Run>,
    /// What the body was painted when this was built, so a new choice rebuilds it.
    painted: Option<gizmo_nfs::Colour>,
    /// Whether a texture pack was available when this was built. When one arrives later the mesh
    /// has to be rebuilt, and the key alone would not say so — it names the *parts*, not their skin.
    textured: bool,
    /// The same vertices as a line list: three edges per triangle. Built with the mesh rather than
    /// on the first press of the wireframe button, because it is 2× the index memory and 0 ms of
    /// anyone's frame — a car's stock configuration is ~50k triangles, so 1.2 MB.
    edges: wgpu::Buffer,
    edge_count: u32,
    /// The same positions again, carrying the wireframe's flat ink where the shaded copy carries a
    /// normal. One buffer cannot be both, and the alternative — a second uniform and a second bind
    /// per draw — is more moving parts for the same 12 bytes a vertex.
    wire_vertices: wgpu::Buffer,
    /// Bounds in the file's frame, for framing the camera and for the overlay.
    pub bounds: ([f32; 3], [f32; 3]),
    /// What is in the buffer, so an unchanged selection is not re-uploaded.
    key: MeshKey,
}

/// Identifies what a mesh was built from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeshKey {
    /// One solid, by the offset of its chunk.
    Solid(usize),
    /// The showroom configuration — what `select_stock_car` picks.
    StockCar,
    /// The assembly tab's build, by a hash of which parts are mounted: the set is what changes, and
    /// an offset would name only one of them.
    Build(u64),
}

/// The preview renderer, held by the app for the lifetime of the window.
pub struct Preview {
    pipeline: wgpu::RenderPipeline,
    /// Bind group per uploaded texture; `materials[0]` is a white 1×1, so an untextured run and a
    /// textured one go down the same shader path.
    materials: Vec<wgpu::BindGroup>,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The same vertex layout drawn as a line list — the ground grid and the wireframe both.
    lines: wgpu::RenderPipeline,
    /// The ground grid, built once: it does not depend on what is open.
    grid: wgpu::Buffer,
    grid_vertices: u32,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    colour: wgpu::Texture,
    depth: wgpu::TextureView,
    size: [u32; 2],
    /// The egui handle for the colour texture.
    texture_id: egui::TextureId,
    mesh: Option<Mesh>,
}

impl Preview {
    /// Build the pipeline. `None` when eframe is running without a wgpu backend, in which case
    /// the tab says so rather than the app refusing to start.
    pub fn new(state: &RenderState) -> Option<Self> {
        let device = &state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pryhub.preview"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pryhub.preview.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pryhub.preview.uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pryhub.preview.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        // Group 1: the run's texture and its sampler. Separate from the uniforms because it changes
        // per draw where they change per frame.
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pryhub.preview.material.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pryhub.preview.sampler"),
            // Repeat: NFSU2's UVs run outside 0..1 on trim and tyre treads, and clamping smears the
            // edge texel down the whole panel.
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pryhub.preview.layout"),
            bind_group_layouts: &[Some(&layout), Some(&material_layout)],
            immediate_size: 0,
        });
        // The lines carry their own ink and sample nothing, so they get a layout without the
        // material group rather than a white texture bound for no reason.
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pryhub.preview.line.layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pryhub.preview.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Double-sided: these meshes are authored for a renderer that culls differently, and a
            // missing face reads as a hole in the part — worse than a slightly wrong silhouette.
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // The same vertices, drawn as a line list. A topology is baked into a pipeline, so this is a
        // second one rather than a flag — and the fragment entry is the flat one, because a line has
        // no surface to light.
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pryhub.preview.lines"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // The fallback skin: one white texel. A run with no texture then needs no second pipeline
        // and no branch in the shader — it samples white and keeps its lit base colour.
        // Slot 0: the neutral this preview used for everything before it could tell paint from
        // glass. Nothing reaches for it now except a part with no material table and no group.
        let neutral = srgb_byte(0.62);
        let white = upload_texture(
            device,
            &state.queue,
            1,
            1,
            &[neutral, srgb_byte(0.63), srgb_byte(0.66), 255],
        );
        let materials = vec![device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pryhub.preview.material.white"),
            layout: &material_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&white) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        })];

        let grid_data = ground_grid();
        let grid = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pryhub.preview.grid"),
            contents: bytemuck::cast_slice(&grid_data),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_vertices = grid_data.len() as u32;

        let size = [16, 16];
        let (colour, depth) = targets(device, size);
        let view = colour.create_view(&wgpu::TextureViewDescriptor::default());
        let texture_id = state.renderer.write().register_native_texture(
            device,
            &view,
            wgpu::FilterMode::Linear,
        );
        Some(Self {
            pipeline,
            materials,
            material_layout,
            sampler,
            lines,
            grid,
            grid_vertices,
            uniforms,
            bind_group,
            colour,
            depth,
            size,
            texture_id,
            mesh: None,
        })
    }

    /// What is currently uploaded.
    #[must_use]
    pub fn mesh_key(&self) -> Option<MeshKey> {
        self.mesh.as_ref().map(|m| m.key)
    }

    /// Whether what is uploaded was built without the texture pack that is now available.
    ///
    /// A [`MeshKey`] names which *parts* are drawn, not what they are wearing, so it cannot answer
    /// this: decoding a pack is a job, and the mesh that was built while it was still running is
    /// correct geometry with a white skin.
    #[must_use]
    pub fn needs_skin(&self, have_tpk: bool) -> bool {
        self.mesh.as_ref().is_some_and(|m| have_tpk && !m.textured)
    }

    /// Whether the body is wearing a different colour from the one asked for.
    #[must_use]
    pub fn needs_paint(&self, paint: Option<gizmo_nfs::Colour>) -> bool {
        self.mesh.as_ref().is_some_and(|m| m.painted != paint)
    }

    /// Drop what is uploaded, so the next frame re-reads the document.
    ///
    /// A [`MeshKey`] names an offset inside *a* file, not inside a particular one, and the renderer
    /// outlives the document by design — it holds a pipeline, not a car. So a second file whose
    /// selection lands on the same key (and `StockCar` is the key every freshly opened file starts
    /// on) would match what is already uploaded and the tab would go on drawing the previous car.
    /// The document knows when it has been replaced; the renderer cannot, so it is told.
    pub fn forget_mesh(&mut self) {
        self.mesh = None;
    }

    /// Bounds of the uploaded mesh, in the file's frame.
    #[must_use]
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.mesh.as_ref().map(|m| m.bounds)
    }

    /// Triangles currently uploaded.
    #[must_use]
    pub fn triangles(&self) -> u32 {
        self.mesh.as_ref().map_or(0, |m| m.index_count / 3)
    }

    /// Upload a set of parts as the mesh to show.
    pub fn upload(
        &mut self,
        state: &RenderState,
        key: MeshKey,
        parts: &[&NfsMeshPart],
        tpk: Option<&gizmo_nfs::Tpk>,
        paint: Option<gizmo_nfs::Colour>,
    ) {
        let (mut vertices, mut indices) = (Vec::new(), Vec::<u32>::new());
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        // One bind group per distinct texture actually used, and a map from hash to its slot so a
        // skin shared by six panels is uploaded once.
        self.materials.truncate(1);
        let mut slot_of: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        // Untextured runs get their material group's colour — the same one `ug2 export` writes into
        // the MTL, so the viewport and the exported car are the same car. It arrives as a 1×1
        // texture rather than as a uniform: the sampling path already exists and a flat colour is
        // just a very small skin.
        let mut tint_of: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
        let mut runs: Vec<Run> = Vec::new();

        for part in parts {
            let group = gizmo_nfs::parts::group_of(&part.name);
            let apply = should_place(&part.transform, &part_centroid(part));
            let base = vertices.len() as u32;
            for (i, p) in part.positions.iter().enumerate() {
                let position = place_point(&part.transform, *p, apply);
                let normal = part
                    .normals
                    .get(i)
                    .map(|n| place_dir(&part.transform, *n, apply))
                    .unwrap_or([0.0, 0.0, 1.0]);
                for k in 0..3 {
                    lo[k] = lo[k].min(position[k]);
                    hi[k] = hi[k].max(position[k]);
                }
                vertices.push(Vertex {
                    position,
                    normal,
                    uv: part.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                });
            }
            let first = indices.len() as u32;
            indices.extend(part.indices.iter().map(|i| base + i));

            // The part's own material table splits those indices; a part without one is a single
            // run with no texture.
            let plain = {
                let rgb = gizmo_nfs::export::material::group_colour(group);
                // A chosen colour replaces the paint group's own and nothing else: glass stays
                // glass, a tail light stays red. NFSU2 paints exactly the panels this group names.
                let key = match paint {
                    Some(c) if group == gizmo_nfs::parts::Grp::Paint => [c.red, c.green, c.blue],
                    _ => [srgb_byte(rgb[0]), srgb_byte(rgb[1]), srgb_byte(rgb[2])],
                };
                match tint_of.get(&key) {
                    Some(slot) => *slot,
                    None => {
                        let view = upload_texture(
                            &state.device,
                            &state.queue,
                            1,
                            1,
                            &[key[0], key[1], key[2], 255],
                        );
                        self.materials.push(state.device.create_bind_group(
                            &wgpu::BindGroupDescriptor {
                                label: Some("pryhub.preview.tint"),
                                layout: &self.material_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                                    },
                                ],
                            },
                        ));
                        let slot = self.materials.len() - 1;
                        tint_of.insert(key, slot);
                        slot
                    }
                }
            };
            if part.materials.is_empty() {
                runs.push(Run { start: first, count: part.indices.len() as u32, material: plain });
                continue;
            }
            for range in &part.materials {
                let material = tpk
                    .and_then(|t| t.texture(range.hash))
                    .map(|tex| {
                        *slot_of.entry(range.hash.0).or_insert_with(|| {
                            let view = upload_texture(
                                &state.device,
                                &state.queue,
                                tex.width,
                                tex.height,
                                &tex.rgba,
                            );
                            self.materials.push(state.device.create_bind_group(
                                &wgpu::BindGroupDescriptor {
                                    label: Some("pryhub.preview.material"),
                                    layout: &self.material_layout,
                                    entries: &[
                                        wgpu::BindGroupEntry {
                                            binding: 0,
                                            resource: wgpu::BindingResource::TextureView(&view),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 1,
                                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                                        },
                                    ],
                                },
                            ));
                            self.materials.len() - 1
                        })
                    })
                    .unwrap_or(plain);
                // The run's offsets are into the part's own index list, so they shift by where that
                // list landed in the shared buffer.
                runs.push(Run {
                    start: first + range.index_offset as u32,
                    count: range.index_count as u32,
                    material,
                });
            }
        }
        if vertices.is_empty() || indices.is_empty() {
            self.mesh = None;
            return;
        }
        // Three edges per triangle. Not deduplicated: a shared edge drawn twice costs one more line
        // and looks identical, where a hash set over 150k pairs would cost the upload it saves.
        let ink = linear(token_rgb(theme::token::NEUTRAL_800));
        let mut edges = Vec::with_capacity(indices.len() * 2);
        for tri in indices.chunks_exact(3) {
            edges.extend_from_slice(&[tri[0], tri[1], tri[1], tri[2], tri[2], tri[0]]);
        }
        // The wireframe wants a flat ink where the surface wants a normal, and both read the same
        // vertex buffer — so the edges get their own copy of the positions, carrying the ink.
        let wire_vertices: Vec<Vertex> =
            vertices.iter().map(|v| Vertex { position: v.position, normal: ink, uv: [0.0, 0.0] }).collect();
        let device = &state.device;
        let mesh = Mesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pryhub.preview.vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pryhub.preview.indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: indices.len() as u32,
            runs,
            textured: tpk.is_some(),
            painted: paint,
            edges: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pryhub.preview.edges"),
                contents: bytemuck::cast_slice(&edges),
                usage: wgpu::BufferUsages::INDEX,
            }),
            edge_count: edges.len() as u32,
            wire_vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pryhub.preview.wire_vertices"),
                contents: bytemuck::cast_slice(&wire_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            bounds: (lo, hi),
            key,
        };
        self.mesh = Some(mesh);
    }

    /// Render one frame at `size` physical pixels and return the egui texture to draw.
    pub fn render(
        &mut self,
        state: &RenderState,
        size: [u32; 2],
        clip_from_world: M4,
        wire: bool,
    ) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        if size != self.size {
            self.resize(state, size);
        }
        let queue = &state.queue;
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                clip_from_world,
                // The design's warm key from above-front-left; the fill is baked into the shader.
                key_dir: [0.45, 0.35, 0.82, 0.0],
            }),
        );

        let view = self.colour.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pryhub.preview") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pryhub.preview.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The page colour, so the viewport sits in the interface rather than on it.
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.925, g: 0.921, b: 0.921, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // The ground first, so the car occludes it rather than the other way round.
            pass.set_pipeline(&self.lines);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.grid.slice(..));
            pass.draw(0..self.grid_vertices, 0..1);

            if let Some(mesh) = &self.mesh {
                if wire {
                    pass.set_vertex_buffer(0, mesh.wire_vertices.slice(..));
                    pass.set_index_buffer(mesh.edges.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.edge_count, 0, 0..1);
                } else {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    // One draw per material run, in the order the file lists them.
                    for run in &mesh.runs {
                        let Some(material) = self.materials.get(run.material) else { continue };
                        pass.set_bind_group(1, material, &[]);
                        pass.draw_indexed(run.start..run.start + run.count, 0, 0..1);
                    }
                }
            }
        }
        queue.submit(Some(encoder.finish()));
        self.texture_id
    }

    /// Rebuild the render targets and re-register the texture with egui.
    fn resize(&mut self, state: &RenderState, size: [u32; 2]) {
        let (colour, depth) = targets(&state.device, size);
        let view = colour.create_view(&wgpu::TextureViewDescriptor::default());
        state.renderer.write().update_egui_texture_from_wgpu_texture(
            &state.device,
            &view,
            wgpu::FilterMode::Linear,
            self.texture_id,
        );
        self.colour = colour;
        self.depth = depth;
        self.size = size;
    }
}

/// The colour target's format. Rgba8 unorm-srgb matches what egui expects to sample.
/// Put RGBA8 pixels on the GPU and hand back a view of them.
///
/// `Rgba8UnormSrgb` because the TPK's pixels are sRGB and the target is too: sampling them as
/// linear would wash every panel out by exactly one gamma.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pryhub.preview.skin"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width * 4),
            rows_per_image: Some(size.height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A palette token as three 0..1 sRGB components.
fn token_rgb(c: egui::Color32) -> [f32; 3] {
    [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0]
}

/// linear → sRGB, as a byte.
///
/// [`gizmo_nfs::export::material::group_colour`] hands out shading *coefficients* — the numbers a
/// shader multiplies by, and what the old flat `0.62` was. Storing one straight into an
/// `Rgba8UnormSrgb` texture would have the sampler linearise it a second time and every panel would
/// come back a third as bright, which is exactly how the first attempt painted a silver 240SX navy.
fn srgb_byte(linear: f32) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let encoded =
        if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (encoded * 255.0).round() as u8
}

/// sRGB → linear. The target is `Rgba8UnormSrgb`, so what a shader writes is taken as linear light
/// and encoded on the way out; handing it a palette value straight would draw every line pale.
fn linear(rgb: [f32; 3]) -> [f32; 3] {
    let f = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(rgb[0]), f(rgb[1]), f(rgb[2])]
}

/// The design's ground plane: 1 m cells over ±8 m, in the file's own frame (Z-up, so the ground is
/// `z = 0`), with the two centre lines a step darker the way a grid helper draws its axes.
///
/// Fixed rather than fitted to the model: it is a *scale*, and a grid that resized itself with the
/// selection would stop being one — a wheel and a whole car would both sit on ten squares.
fn ground_grid() -> Vec<Vertex> {
    const HALF: i32 = 8;
    let line = linear(token_rgb(theme::token::NEUTRAL_400));
    let axis = linear(token_rgb(theme::token::NEUTRAL_500));
    let mut out = Vec::with_capacity((HALF as usize * 2 + 1) * 4);
    for i in -HALF..=HALF {
        let t = i as f32;
        let e = HALF as f32;
        let ink = if i == 0 { axis } else { line };
        out.push(Vertex { position: [-e, t, 0.0], normal: ink, uv: [0.0, 0.0] });
        out.push(Vertex { position: [e, t, 0.0], normal: ink, uv: [0.0, 0.0] });
        out.push(Vertex { position: [t, -e, 0.0], normal: ink, uv: [0.0, 0.0] });
        out.push(Vertex { position: [t, e, 0.0], normal: ink, uv: [0.0, 0.0] });
    }
    out
}

const TARGET: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn targets(device: &wgpu::Device, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
    let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
    let colour = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pryhub.preview.colour"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("pryhub.preview.depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    (colour, depth)
}

/// Lambert with a warm key and a cool fill — the same two-light rig the rest of the project uses,
/// so a part looks here the way it looks in the engine.
const SHADER: &str = r#"
struct Uniforms {
    clip_from_world: mat4x4<f32>,
    key_dir: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@group(1) @binding(0) var skin: texture_2d<f32>;
@group(1) @binding(1) var skin_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = u.clip_from_world * vec4<f32>(position, 1.0);
    out.normal = normal;
    out.uv = uv;
    return out;
}

// Lines carry their ink in the slot a surface uses for its normal: a grid line and a wireframe edge
// want different colours in the same pass, and putting the colour in the vertex costs one shader
// entry where a uniform would cost a second write and a second bind per draw.
@fragment
fn fs_line(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.normal, 1.0);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // Two-sided: these parts are authored for a renderer that culls differently, so a face turned
    // away is lit as if it were turned towards us rather than left black.
    let n = normalize(in.normal);
    let key = abs(dot(n, normalize(u.key_dir.xyz)));
    let fill = abs(dot(n, normalize(vec3<f32>(-0.4, -0.6, 0.3))));
    let lit = 0.18 + 0.72 * key + 0.22 * fill;
    // Whatever this run wears: a decoded texture, or the 1×1 of its material group's colour. Both
    // arrive the same way, so there is no branch here and no second pipeline.
    let base = textureSample(skin, skin_sampler, in.uv).rgb;
    return vec4<f32>(base * lit, 1.0);
}
"#;
