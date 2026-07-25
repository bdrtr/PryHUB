//! The 3D tab: the selected part, orbited.
//!
//! What it shows follows the selection, which is the design's rule for the whole centre area: a
//! chunk inside a solid shows that solid's mesh; anything else shows the whole file. The frame is
//! the file's own — Z-up, one unit to the metre — so the viewport agrees with the status bar and
//! with what `ug2 export` writes.
//!
//! Everything here is the *chrome* over the picture: the HUD block, the camera toolbar, the Z-UP
//! badge. The picture itself is [`crate::gpu::preview`].

use crate::app::PryHub;
use crate::gpu::math;
use crate::gpu::preview::MeshKey;
use crate::theme::{self, token};
use crate::widget::{self, Seg};
use egui::{pos2, vec2, Color32, Rect, Vec2};

/// How far every floating element sits from the pane's edge — the design's `top/right/bottom/left:
/// 12px` on all four corners of the viewport.
const INSET: f32 = 12.0;

/// Where the camera is looking from.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    /// Distance from the target, in metres.
    pub distance: f32,
    pub target: [f32; 3],
    /// Set when the mesh changed and the camera should re-frame it.
    pub framed: Option<MeshKey>,
    /// The design opens with the viewport turning (`autoRot: true`): a still three-quarter view
    /// reads as a picture of a car, a turning one as a model of it.
    pub auto_rotate: bool,
}

impl Default for Camera {
    fn default() -> Self {
        // A three-quarter view from above the front — the angle the design's viewport shows, and
        // the one that reads as "a car" rather than as a silhouette.
        Self {
            yaw: 0.9,
            pitch: 0.35,
            distance: 6.0,
            target: [0.0; 3],
            framed: None,
            auto_rotate: true,
        }
    }
}

impl Camera {
    /// The design's `maxPolarAngle: PI * 0.52` — the eye may dip a hair under the horizon and no
    /// further, so a part is never seen from beneath the ground it stands on.
    const MIN_PITCH: f32 = -std::f32::consts::PI * 0.02;
    const MAX_PITCH: f32 = 1.5;
    /// `autoRotateSpeed: 0.9` in three.js's units — 0.9 turns per minute, in radians a second.
    const SPIN: f32 = 0.094;

    /// Put the whole of `bounds` in view.
    fn frame(&mut self, bounds: ([f32; 3], [f32; 3])) {
        let (lo, hi) = bounds;
        let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let extent = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let radius = (extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt() * 0.5;
        self.target = centre;
        self.distance = (radius * 2.4).clamp(0.4, 400.0);
    }

    /// Back to the view the tab opens at. Distance and target are not restored but *forgotten*:
    /// clearing `framed` re-frames whatever is loaded, which is what "reset" means for a mesh
    /// whose size the default was never chosen for.
    fn home(&mut self) {
        let opening = Self::default();
        self.yaw = opening.yaw;
        self.pitch = opening.pitch;
        self.framed = None;
    }

    /// The clip-from-world matrix for an image of `aspect`.
    fn clip_from_world(&self, aspect: f32) -> math::M4 {
        let eye = math::orbit_eye(self.target, self.yaw, self.pitch, self.distance);
        let near = (self.distance * 0.01).max(0.01);
        let far = self.distance * 10.0 + 10.0;
        math::mul(
            // 42°, the design's own lens. A wider one bends a bumper at the edge of the frame.
            math::perspective(0.733, aspect, near, far),
            math::look_at(eye, self.target, [0.0, 0.0, 1.0]),
        )
    }
}

/// Draw the tab.
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let Some(state) = app.render_state.clone() else {
        widget::note_box(ui, t.no_gpu);
        return;
    };
    if app.preview.is_none() {
        app.preview = crate::gpu::preview::Preview::new(&state);
    }
    // The document is taken by `Arc` rather than borrowed: the preview below is a `&mut` into the
    // same app, and one clone of a pointer is cheaper than threading two field borrows through.
    let Some(doc) = app.doc.clone() else {
        widget::note_box(ui, t.no_file);
        return;
    };
    // The skin. Decoding a pack is a job, so this is `None` for the first frames after a file opens
    // and the car is grey until it lands — then the mesh is rebuilt with it. Asked for here rather
    // than only on the texture tab: the whole point of the build is what the car will look like.
    app.want_textures();
    let tpk = app.textures.ready().cloned();
    let paint = app.paint;

    // Taken before the renderer is borrowed mutably: it reads the whole app, and the borrow below
    // lasts for the rest of the function.
    let building = app.tab == crate::app::Tab::Assembly;
    let build = building.then(|| crate::panels::assembly::build_key(app, &doc.parts));
    let mounted = building.then(|| crate::panels::assembly::mounted_indices(app, &doc.parts));
    let Some(preview) = app.preview.as_mut() else {
        widget::note_box(ui, t.no_gpu);
        return;
    };

    // What to show. On the assembly tab it is the build and nothing else: that tab is *about* the
    // set of mounted parts, and following the tree selection there would answer a question nobody
    // asked. Everywhere else it is the solid the selection sits in, else the whole file.
    let solid = if building { None } else { app.selection.and_then(|o| doc.solid_of(o)) };
    let key = match build {
        Some(hash) => MeshKey::Build(hash),
        None => solid.map_or(MeshKey::StockCar, |s| MeshKey::Solid(s.offset)),
    };
    if preview.mesh_key() != Some(key)
        || preview.needs_skin(tpk.is_some())
        || preview.needs_paint(paint)
    {
        let parts: Vec<&gizmo_nfs::NfsMeshPart> = match solid {
            // One solid: match it by the name in its header, which is what ties a chunk in the
            // tree to a parsed part.
            Some(s) => {
                let name = s
                    .find(gizmo_nfs::geometry::format::SOLID_HEADER)
                    .map(|h| gizmo_nfs::geometry::part_name(h.data(&doc.bytes)))
                    .unwrap_or_default();
                doc.parts.iter().filter(|p| p.name == name).collect()
            }
            // Nothing solid selected: the showroom car, not every part in the file. Drawing all
            // 609 would stack two dozen bumpers and four widebodies on top of each other — true
            // to the bytes, useless as a picture.
            None => match &mounted {
                Some(ix) => ix.iter().map(|&i| &doc.parts[i]).collect(),
                None => gizmo_nfs::select_stock_car(&doc.parts),
            },
        };
        Counts {
            vertices: parts.iter().map(|p| p.positions.len() as u32).sum(),
            submeshes: parts.len() as u32,
        }
        .store(ui.ctx());
        preview.upload(&state, key, &parts, tpk.as_deref(), paint);
        app.camera.framed = None;
    }
    if app.camera.framed != Some(key) {
        if let Some(bounds) = preview.bounds() {
            app.camera.frame(bounds);
        }
        app.camera.framed = Some(key);
    }

    // The viewport itself. The design's 3D pane is `inset: 0` — the picture butts straight against
    // the tab strip's rule and the tree and inspector dividers — so it reaches back out through the
    // centre panel's own margin (`workspace::panel_frame`, 6 × 4) and through the gap the layout
    // leaves under the rule above it.
    let avail = ui.available_rect_before_wrap();
    let gap = ui.spacing().item_spacing.y;
    let pane = Rect::from_min_max(
        pos2(avail.left() - 6.0, avail.top() - gap),
        pos2(avail.right() + 6.0, avail.bottom() + 4.0),
    )
    .intersect(ui.clip_rect());
    let response = ui.allocate_rect(pane, egui::Sense::click_and_drag());
    // The picture is painted into a slot reserved now and filled once the camera has settled, so
    // the toolbar below draws — and hit-tests — on top of it.
    let image_slot = ui.painter().add(egui::Shape::Noop);
    let bar = toolbar(app, ui, pane);

    // egui picks the click and the drag targets independently, so a button on top takes the press
    // while the pane underneath would still take the motion: a press that began on the toolbar is
    // not an orbit.
    let from_bar = ui.input(|i| i.pointer.press_origin()).is_some_and(|p| bar.contains(p));
    let drag = response.drag_delta();
    if drag != Vec2::ZERO && !from_bar {
        app.camera.yaw -= drag.x * 0.01;
        app.camera.pitch =
            (app.camera.pitch + drag.y * 0.01).clamp(Camera::MIN_PITCH, Camera::MAX_PITCH);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            app.camera.distance = (app.camera.distance * (1.0 - scroll * 0.002)).clamp(0.2, 500.0);
        }
    }
    // Auto-rotation yields to the hand on it, the way the design's OrbitControls does.
    if app.camera.auto_rotate && !response.dragged() {
        app.camera.yaw += Camera::SPIN * ui.input(|i| i.stable_dt);
        ui.ctx().request_repaint();
    }

    let ppp = ui.ctx().pixels_per_point();
    let size = [(pane.width() * ppp) as u32, (pane.height() * ppp) as u32];
    let aspect = pane.width().max(1.0) / pane.height().max(1.0);
    let Some(preview) = app.preview.as_mut() else { return };
    let texture = preview.render(&state, size, app.camera.clip_from_world(aspect), app.wire);
    ui.painter().set(
        image_slot,
        egui::Shape::image(
            texture,
            pane,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        ),
    );

    overlay(app, ui, pane);
}

/// The camera's own controls, top-right over the picture.
///
/// Returns the rectangle they occupy, which is what tells the orbit above to keep its hands off a
/// drag that started here.
fn toolbar(app: &mut PryHub, ui: &mut egui::Ui, pane: Rect) -> Rect {
    let t = app.lang.strings();
    let bar = Rect::from_min_max(
        pos2(pane.left() + INSET, pane.top() + INSET),
        pos2(pane.right() - INSET, pane.bottom() - INSET),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(egui::Layout::right_to_left(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Laid out from the right, so the design's row order — Wireframe, Rotate, Reset — is
            // added back to front.
            if widget::seg_button(ui, t.reset, false, Seg::Small).clicked() {
                app.camera.home();
                ui.ctx().request_repaint();
            }
            let spinning = app.camera.auto_rotate;
            if widget::seg_button(ui, t.rotate, spinning, Seg::Small).clicked() {
                app.camera.auto_rotate = !spinning;
            }
            let wire = app.wire;
            if widget::seg_button(ui, t.wire, wire, Seg::Small).clicked() {
                app.wire = !wire;
            }
            ui.min_rect()
        },
    )
    .inner
}

/// The corner readouts the design puts over the viewport: what is shown, what it is made of, which
/// way is up, and how to move the camera.
fn overlay(app: &PryHub, ui: &egui::Ui, rect: Rect) {
    let t = app.lang.strings();
    let p = ui.painter();
    let Some(preview) = &app.preview else { return };
    let fill = chip_fill();

    // Top-left: the name in full ink, then two lines of numbers under it. The block is drawn even
    // when there is no mesh — a solid that parsed to nothing still has a name worth reading.
    let selected =
        app.selected_model().and_then(|m| m.summary.clone()).or_else(|| app.selected_solid_name());
    let name = match preview.mesh_key() {
        Some(MeshKey::StockCar) => t.stock_car.to_string(),
        Some(MeshKey::Solid(_)) => selected.unwrap_or_default(),
        // The build says what it *is* rather than which chunk it came from: it came from a dozen.
        Some(MeshKey::Build(_)) => app
            .doc
            .as_ref()
            .map_or_else(|| t.stock_car.to_string(), |doc| doc.file_name()),
        None => selected.unwrap_or_else(|| t.stock_car.to_string()),
    };
    let small = theme::font::mono(10.0);
    let secondary = theme::muted(60);
    let mut lines = vec![p.layout_no_wrap(name, theme::font::mono(11.0), token::TEXT)];
    lines.push(p.layout_no_wrap(
        Counts::read(ui.ctx()).line(preview.triangles()),
        small.clone(),
        secondary,
    ));
    if let Some((lo, hi)) = preview.bounds() {
        lines.push(p.layout_no_wrap(
            format!(
                "{} {:.2} × {:.2} × {:.2} m",
                t.bbox,
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2]
            ),
            small,
            secondary,
        ));
    }

    // `padding: 6px 10px`, with the accent border adding its own 2 px on the left.
    const LEAD: f32 = 2.0;
    let text_w = lines.iter().fold(0.0_f32, |w, g| w.max(g.size().x));
    let text_h: f32 = lines.iter().map(|g| g.size().y).sum::<f32>() + LEAD * (lines.len() - 1) as f32;
    let chip = Rect::from_min_size(
        rect.left_top() + Vec2::splat(INSET),
        vec2(token::RULE + 10.0 + text_w + 10.0, text_h + 12.0),
    );
    p.rect_filled(chip, 0.0_f32, fill);
    theme::draw::accent_left_bar(p, chip);
    let mut y = chip.top() + 6.0;
    for line in lines {
        let height = line.size().y;
        p.galley(pos2(chip.left() + token::RULE + 10.0, y), line, token::TEXT);
        y += height + LEAD;
    }

    // Bottom-left: the badge that makes the status bar's `1u = 1m · Z↑` visible in the picture, and
    // the hint beside it — outside the chip, and in the body face rather than the mono one.
    const SQUARE: f32 = 8.0;
    let label = p.layout_job(theme::tracked("Z-UP", 0.08, theme::font::mono(10.0), token::TEXT));
    let hint = p.layout_no_wrap(t.drag_hint.to_owned(), theme::font::body(10.5), theme::muted(55));
    // `padding: 4px 9px`, around a 8 px square and its 5 px gap.
    let height = label.size().y.max(SQUARE) + 8.0;
    let badge = Rect::from_min_size(
        pos2(rect.left() + INSET, rect.bottom() - INSET - height),
        vec2(9.0 + SQUARE + 5.0 + label.size().x + 9.0, height),
    );
    p.rect_filled(badge, 0.0_f32, fill);
    p.rect_filled(
        Rect::from_center_size(
            pos2(badge.left() + 9.0 + SQUARE * 0.5, badge.center().y),
            Vec2::splat(SQUARE),
        ),
        0.0_f32,
        token::ACCENT,
    );
    let label_at = pos2(
        badge.left() + 9.0 + SQUARE + 5.0,
        badge.center().y - label.size().y * 0.5,
    );
    // The hint is not in the chip: the design leaves it on the picture, 10 px along and centred
    // against the badge rather than sitting on the pane's own bottom edge.
    let hint_at = pos2(badge.right() + 10.0, badge.center().y - hint.size().y * 0.5);
    p.galley(label_at, label, token::TEXT);
    p.galley(hint_at, hint, theme::muted(55));
}

/// What every floating block over the viewport is backed with: `color-mix(bg 88%, transparent)`,
/// so the picture shows through it without the type having to fight the car behind it.
fn chip_fill() -> Color32 {
    token::BG.gamma_multiply(0.88)
}

/// What the HUD says the mesh is made of.
///
/// [`crate::gpu::preview::Preview`] counts the indices it uploaded and nothing else, so the vertex
/// and part totals are taken where the part list is still in hand — at upload — and parked in
/// egui's memory rather than recounted every frame.
#[derive(Clone, Copy, Default)]
struct Counts {
    vertices: u32,
    submeshes: u32,
}

impl Counts {
    fn id() -> egui::Id {
        egui::Id::new("viewport3d.counts")
    }

    fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.insert_temp(Self::id(), self);
        });
    }

    fn read(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(Self::id())).unwrap_or_default()
    }

    /// The design's `2,190 vtx · 4,268 tri · 3 submesh` — three words it leaves in English in both
    /// languages, so they stay as short as the numbers they label.
    fn line(self, triangles: u32) -> String {
        if self.vertices == 0 {
            return format!("{} tri", grouped(triangles));
        }
        format!(
            "{} vtx · {} tri · {} submesh",
            grouped(self.vertices),
            grouped(triangles),
            grouped(self.submeshes)
        )
    }
}

/// Thousands, the way every count in the design is written: `4,268`.
pub(crate) fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.char_indices() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(4268), "4,268");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    /// Without a part list the HUD prints what it can rather than a made-up zero.
    #[test]
    fn an_unknown_mesh_prints_only_its_triangles() {
        assert_eq!(Counts::default().line(4268), "4,268 tri");
        assert_eq!(
            Counts { vertices: 2190, submeshes: 3 }.line(4268),
            "2,190 vtx · 4,268 tri · 3 submesh"
        );
    }

    /// The design's `maxPolarAngle` stops the eye at the ground; reset returns to the opening view.
    #[test]
    fn reset_returns_to_the_opening_angles() {
        let mut cam = Camera { yaw: 3.0, pitch: 1.2, framed: Some(MeshKey::StockCar), ..Camera::default() };
        cam.home();
        assert_eq!((cam.yaw, cam.pitch), (Camera::default().yaw, Camera::default().pitch));
        assert!(cam.framed.is_none(), "re-frames the mesh it is actually looking at");
        // Both sides are constants, so this is checked when the crate is built rather than when the
        // test is run — the floor cannot be edited past the ground and still compile.
        const { assert!(Camera::MIN_PITCH < 0.0 && Camera::MIN_PITCH > -0.1) };
    }
}
