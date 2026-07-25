//! The workspace: five regions over one open file.
//!
//! Panel registration order is significant in egui — the bottom log is registered before the side
//! panels so it spans the full width beneath them, matching the design's layout.

use crate::app::{PryHub, Tab};
use crate::panels;
use crate::theme::{self, token};
use egui::{Margin, Rect, RichText, Sense};

/// The design's column borders, in the `i8` egui stores margins as: `token::RULE`, kept out of the
/// content so a row's wash and the scrollbar stop where the border starts (`box-sizing: border-box`).
const BORDER: i8 = 2;

/// How far a hand-drawn border has to reach past the seam between two panels: egui still paints a
/// hairline centred on it while the splitter is hovered or dragged, and half of that falls outside
/// the column the border belongs to.
const SEAM: f32 = 0.5;

pub fn show(app: &mut PryHub, ui: &mut egui::Ui) {
    // Actions the panels report; applied after drawing so no panel holds a mutable borrow of the
    // app while it draws.
    let mut select: Option<usize> = None;
    let mut select_byte: Option<usize> = None;
    let mut fold: Option<usize> = None;
    let density = app.density;

    let log_rect = egui::Panel::bottom("log")
        .resizable(true)
        .default_size(density.log_height())
        .show_separator_line(false)
        .frame(panel_frame(Margin { top: BORDER, ..Margin::ZERO }))
        .show_inside(ui, |ui| {
            flush(ui);
            panels::log::caption(app, ui);
            if let Some(offset) = panels::log::show(app, ui) {
                select = Some(offset);
            }
        })
        .response
        .rect;

    let tree_rect = egui::Panel::left("tree")
        .resizable(true)
        .default_size(density.tree_width())
        .show_separator_line(false)
        .frame(panel_frame(Margin { right: BORDER, ..Margin::ZERO }))
        .show_inside(ui, |ui| {
            flush(ui);
            panels::tree::caption(app, ui);
            match panels::tree::show(app, ui) {
                Some(panels::tree::Hit::Select(offset)) => select = Some(offset),
                // Applied after the panel closure: it is holding the app immutably while it draws.
                Some(panels::tree::Hit::Toggle(offset)) => fold = Some(offset),
                None => {}
            }
        })
        .response
        .rect;

    let inspector_rect = egui::Panel::right("inspector")
        .resizable(true)
        .default_size(density.inspector_width())
        .show_separator_line(false)
        .frame(panel_frame(Margin { left: BORDER, ..Margin::ZERO }))
        .show_inside(ui, |ui| {
            flush(ui);
            panels::inspector::caption(app, ui);
            panels::inspector::show(app, ui);
        })
        .response
        .rect;

    egui::CentralPanel::default().frame(panel_frame(Margin::ZERO)).show_inside(ui, |ui| {
        flush(ui);
        tabs(app, ui);
        match app.tab {
            // Each pane states its own padding, as the design does — the hex dump's 10 × 12 and the
            // texture grid's 16 are nothing like each other, and the 3D pane is `inset: 0`: the
            // canvas fills the region and the HUD floats over it.
            Tab::Hex => {
                egui::Frame::new().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                    if let Some(byte) = panels::hex::show(app, ui) {
                        select_byte = Some(byte);
                    }
                });
            }
            Tab::ThreeD => panels::viewport3d::show(app, ui),
            // The build's list sits between the picture and the inspector, the way the design puts
            // it: the viewport keeps the rest.
            Tab::Assembly => {
                egui::Panel::right("mounted")
                    .exact_size(panels::assembly::PANEL_W)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(token::SURFACE))
                    .show_inside(ui, |ui| panels::assembly::panel(app, ui));
                panels::viewport3d::show(app, ui);
            }
            Tab::Texture => {
                egui::Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
                    panels::texture::show(app, ui);
                });
            }
        }
    });

    // The columns' own borders, drawn last: a panel laid out later would otherwise paint over the
    // edge of one laid out earlier, and the log's runs the full width under all three. Each is the
    // design's `border-right` / `border-left` / `border-top`, inside its own column.
    border_v(ui, tree_rect.right() - token::RULE..=tree_rect.right() + SEAM, tree_rect);
    border_v(ui, inspector_rect.left() - SEAM..=inspector_rect.left() + token::RULE, inspector_rect);
    border_h(ui, log_rect.top() - SEAM..=log_rect.top() + token::RULE, log_rect);

    if let Some(offset) = fold {
        fold_container(app, offset);
    }
    // A tree/log click selects directly; a hex click selects whichever chunk owns that byte.
    if let Some(offset) = select {
        app.select(offset);
    } else if let Some(byte) = select_byte {
        if let Some(owner) = app.doc.as_ref().and_then(|d| d.owner_of_byte(byte)).map(|n| n.offset) {
            // Deliberately not `select`: the hex view is already showing this byte, and yanking
            // it to a new scroll position under the cursor would fight the user.
            app.selection = Some(owner);
        }
    } else {
        // The scroll request lives exactly one frame — otherwise the hex view would refuse to be
        // scrolled by hand.
        app.scroll_hex_to = None;
    }
}

/// The centre area's tab bar: 3D · HEX · TEXTURE on the design's surface band, the selected one cut
/// through to the page colour and underlined on the band's own rule.
fn tabs(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    egui::Frame::new().fill(token::SURFACE).show(ui, |ui| {
        // The band runs the full width of the column whether or not anything is selected.
        let width = ui.available_width();
        ui.set_min_width(width);
        // The tabs abut — they are direct children of the strip, and their own 16 px of padding is
        // the only gap between the labels.
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut active = None;
        ui.horizontal(|ui| {
            for (tab, label) in
                [
                    (Tab::ThreeD, t.tab_3d),
                    (Tab::Hex, t.tab_hex),
                    (Tab::Texture, t.tab_tex),
                    (Tab::Assembly, t.tab_asm),
                ]
            {
                let on = app.tab == tab;
                // The ink and the notch crossfade with the underline's slide, so the three read as
                // one movement.
                let lit =
                    ui.ctx().animate_bool_with_time(ui.id().with(label), on, crate::chrome::MOVE_TIME);
                let colour = theme::muted(65).lerp_to_gamma(token::ACCENT, lit);
                let galley = ui
                    .painter()
                    .layout_job(theme::tracked(label, 0.06, theme::font::heading(11.0), colour));
                // `padding: 8px 16px`, which is also the width the underline has to span.
                let (rect, resp) = ui.allocate_exact_size(
                    galley.size() + egui::vec2(token::SPACE_4 * 2.0, token::SPACE_2 * 2.0),
                    Sense::click(),
                );
                if on {
                    active = Some(rect);
                }
                let p = ui.painter();
                p.rect_filled(rect, 0.0_f32, token::SURFACE.lerp_to_gamma(token::BG, lit));
                // The design gives a tab no hover state at all; a desktop control that answers the
                // pointer with nothing feels broken, and the wash is the design's own.
                if !on && resp.hovered() {
                    p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
                }
                p.galley(rect.center() - galley.size() * 0.5, galley, colour);
                if resp.clicked() {
                    app.tab = tab;
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            if let Some(node) = app.doc.as_ref().zip(app.selection).and_then(|(d, o)| d.node_at(o)) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0); // the meta's own `padding-right`
                    ui.label(
                        RichText::new(format!(
                            "{:#010x} · {}",
                            node.header.id,
                            panels::inspector::chunk_label(node.header.id)
                        ))
                        .font(theme::font::mono(10.5))
                        .color(theme::muted(48)),
                    );
                });
            }
        });

        // The strip's own bottom border, over the surface it sits on — and the accent rule of the
        // selected tab lying exactly on it, which is the design's `margin-bottom: -2px`.
        let band = ui.min_rect();
        theme::draw::rule_h(
            ui.painter(),
            band.left()..=band.right(),
            band.bottom() - token::RULE,
            token::RULE,
            token::DIVIDER_ON_SURFACE,
        );
        if let Some(rect) = active {
            crate::chrome::slide_underline(
                ui,
                egui::Id::new("tab_underline"),
                Rect::from_min_max(
                    egui::pos2(rect.left(), band.top()),
                    egui::pos2(rect.right(), band.bottom()),
                ),
                -token::RULE * 0.5,
            );
        }
    });
}

/// A region's frame: the page colour, and no padding beyond the column's own border.
///
/// Every region states its own padding inside — the caption bands are full-bleed surface, the rows
/// under them run edge to edge, and the centre's three panes are padded nothing like each other.
fn panel_frame(margin: Margin) -> egui::Frame {
    egui::Frame::new().fill(token::BG).inner_margin(margin)
}

/// Take the panel's vertical rhythm out of a region.
///
/// A caption band, the rule that closes it and the content under it are flush in the design;
/// `item_spacing.y` would open the density's padding above *and* below every one of those rules.
/// What a region wants between its own blocks, it says itself.
fn flush(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// A column's border: the divider filling `x`, down the full height of `panel`.
///
/// Hand-drawn because egui's separator is a hairline that swaps to the *text* colour under the
/// pointer and to the page colour while it is dragged — this design has no splitter, so its edge
/// has no states. The resize affordance itself is untouched; only its colour stops moving.
fn border_v(ui: &egui::Ui, x: std::ops::RangeInclusive<f32>, panel: Rect) {
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(*x.start(), panel.top()),
            egui::pos2(*x.end(), panel.bottom()),
        ),
        0.0_f32,
        token::DIVIDER,
    );
}

/// The log's border, drawn for the same reason as [`border_v`]: the divider filling `y`, across the
/// full width of `panel`.
fn border_h(ui: &egui::Ui, y: std::ops::RangeInclusive<f32>, panel: Rect) {
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(panel.left(), *y.start()),
            egui::pos2(panel.right(), *y.end()),
        ),
        0.0_f32,
        token::DIVIDER,
    );
}

/// Fold or unfold a container. Shared with the discovery screen, which draws the same tree and has
/// to answer its caret the same way.
pub(crate) fn fold_container(app: &mut PryHub, offset: usize) {
    if !app.collapsed.remove(&offset) {
        app.collapsed.insert(offset);
    }
}
