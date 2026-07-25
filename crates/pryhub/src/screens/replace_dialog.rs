//! The replace dialog: which image goes into the selected texture, and where the pack lands.
//!
//! Built on the export dialog's shape, and for the same reason — the primary button says what it is
//! about to do before it does it — but with one question the export never has to ask. An export
//! writes a file nothing else reads; this writes a `TEXTURES.BIN`, which is a file the *game* reads,
//! and the choice between a copy and the original is not a preference. So it is a row of its own,
//! it defaults to the copy, and the path it resolves to is on screen before the button is pressed
//! rather than reported in the log afterwards.
//!
//! **The design draws none of this.** Its texture tab is a permanent empty state with no controls at
//! all, and the only write affordance anywhere in the prototype is the CARP screen's Save/Revert
//! pair — beside a note saying the first write feature lands *there*. The vocabulary is borrowed
//! from that pair rather than invented: the same modal frame, the same accent top edge, the same
//! secondary-then-primary footer.

use crate::app::PryHub;
use crate::theme::{self, token};
use egui::{Color32, RichText, Sense, Vec2};

/// The export dialog's measures, which are the design's for a modal — one object at every density.
const WIDTH: f32 = 480.0;
const PAD_X: f32 = 18.0;
const INNER: f32 = WIDTH - PAD_X * 2.0;

/// Draw the dialog if it is open, and act on what was pressed.
pub fn show(app: &mut PryHub, ctx: &egui::Context) {
    if !app.show_replace {
        return;
    }
    let t = app.lang.strings();
    // Everything this dialog is about comes from the pack on screen: the document, its decoded
    // textures, and which of them is selected. Reached by a click on a texture all three are there
    // already — but `--screen replace` raises the flag before the file has even been opened, and
    // opening and decoding are both jobs.
    //
    // So a missing piece is only a reason to give up when nothing is coming. Clearing the flag
    // outright consumed the request a frame or two before the parse it was waiting for, which is the
    // same trap the export dialog carries a note about, and it is worse here because there are two
    // jobs to wait through rather than one.
    let waiting = app.jobs.busy().is_some()
        || matches!(app.textures, crate::app::Textures::Unasked | crate::app::Textures::Decoding);
    let give_up = |app: &mut PryHub| {
        if !waiting {
            app.show_replace = false;
        }
    };
    let (Some(doc), Some(hash)) = (app.doc.clone(), app.texture_selection) else {
        give_up(app);
        return;
    };
    let Some(pack) = doc.pack_path() else {
        give_up(app);
        return;
    };
    let Some(tex) = app.textures.ready().and_then(|p| p.texture(hash).cloned()) else {
        give_up(app);
        return;
    };

    let mut png = std::mem::take(&mut app.replace_png);
    let mut over = app.replace_over;
    let mut close = false;
    let mut go = false;
    let mut browse = false;

    let response = egui::Modal::new(egui::Id::new("replace_dialog"))
        .backdrop_color(token::SCRIM)
        .frame(egui::Frame::new().fill(token::SURFACE).shadow(theme::shadow::LG))
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            let top = ui.cursor().top();
            let x = ui.max_rect().x_range();
            ui.painter().rect_filled(
                egui::Rect::from_x_y_ranges(x, top..=top + 3.0),
                0.0_f32,
                token::ACCENT,
            );
            ui.add_space(3.0);

            header(ui, t.rep_title, &mut close);
            body(ui, app, t, &tex, &pack, &mut png, &mut over, &mut browse);
            footer(ui, t, &tex, png.trim().is_empty(), &mut close, &mut go);
        });

    app.replace_png = png;
    app.replace_over = over;
    if response.should_close() {
        close = true;
    }
    if browse {
        // The chooser is another window and answers frames later; the dialog stays up, and its
        // field fills in when the answer lands.
        app.ask_for_image(ctx);
    }
    if go {
        app.replace_now();
        close = true;
    }
    if close {
        app.show_replace = false;
        // The overwrite flag does not outlive the dialog — see the field's own note. Someone who
        // wrote over a game file once has not thereby asked to do it again.
        app.replace_over = false;
    }
}

/// Title, close button, and the 2 px rule under them.
fn header(ui: &mut egui::Ui, title: &str, close: &mut bool) {
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 16, bottom: 12 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(theme::font::h4()).color(token::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let galley = ui.painter().layout_no_wrap(
                        "×".to_owned(),
                        theme::font::heading(18.0),
                        Color32::PLACEHOLDER,
                    );
                    let (rect, resp) = ui
                        .allocate_exact_size(galley.size() + Vec2::new(16.0, 4.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0_f32, token::WASH_HOVER);
                    }
                    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, token::TEXT);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        *close = true;
                    }
                });
            });
        });
    rule(ui);
}

/// Which texture, which image, and where the pack goes.
#[allow(clippy::too_many_arguments)]
fn body(
    ui: &mut egui::Ui,
    app: &PryHub,
    t: &'static crate::i18n::Strings,
    tex: &gizmo_nfs::NfsTexture,
    pack: &std::path::Path,
    png: &mut String,
    over: &mut bool,
    browse: &mut bool,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 16, bottom: 16 })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            // What is being replaced, stated rather than assumed to be remembered: the dialog
            // covers the pane the user was reading it from.
            group_label(ui, t.tab_tex);
            read_only(
                ui,
                app,
                &format!(
                    "{}  ·  {} × {}  ·  {:?}",
                    if tex.name.is_empty() { format!("{:#010x}", tex.hash.0) } else { tex.name.clone() },
                    tex.width,
                    tex.height,
                    tex.source_format
                ),
            );
            ui.add_space(6.0);
            // The rule a replacement has to keep, where the choice is made. Meeting it as an error
            // afterwards would be this dialog's failure, not the user's.
            ui.label(RichText::new(t.rep_note).font(theme::font::body(10.5)).color(theme::muted(52)));
            ui.add_space(token::SPACE_4);

            group_label(ui, t.rep_png);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = token::SPACE_2;
                // The button is drawn only where there is a chooser to open — the same rule the
                // compare screen's row follows, and for the same reason: a button that reaches for
                // something the machine does not have is worse than no button.
                let has_picker = crate::picker::available();
                let field = if has_picker { INNER - 92.0 } else { INNER };
                crate::widget::input(ui, png, field, "…/texture.png", true);
                if has_picker && crate::widget::button_secondary(ui, t.browse).clicked() {
                    *browse = true;
                }
            });
            ui.add_space(token::SPACE_4);

            group_label(ui, t.rep_target);
            crate::widget::segmented_filling(
                ui,
                egui::Id::new("rep_target"),
                over,
                &[(false, t.rep_copy), (true, t.rep_over)],
                INNER,
            );
            ui.add_space(6.0);
            if *over {
                ui.label(
                    RichText::new(t.rep_over_note)
                        .font(theme::font::body(10.5))
                        .color(token::ACCENT),
                );
                ui.add_space(6.0);
            }
            // The resolved path, before the button rather than in the log after it.
            let path = crate::replace::target(pack, *over)
                .map_or_else(|e| e, |p| p.display().to_string());
            read_only(ui, app, &path);
        });
}

/// Cancel and the primary, over the 2 px rule that closes the dialog.
fn footer(
    ui: &mut egui::Ui,
    t: &'static crate::i18n::Strings,
    tex: &gizmo_nfs::NfsTexture,
    no_image: bool,
    close: &mut bool,
    go: &mut bool,
) {
    rule(ui);
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = token::SPACE_2;
                let name = if tex.name.is_empty() {
                    format!("{:#010x}", tex.hash.0)
                } else {
                    tex.name.clone()
                };
                // Nothing to write until there is an image; the primary says what it would do and
                // takes the disabled treatment until it can.
                ui.add_enabled_ui(!no_image, |ui| {
                    if crate::widget::button_primary(ui, &format!("{} · {name}", t.rep_go)).clicked()
                    {
                        *go = true;
                    }
                });
                if crate::widget::button_secondary(ui, t.exp_cancel).clicked() {
                    *close = true;
                }
            });
        });
}

/// The design's 11 px / 60 % label over a row.
fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font::body(11.0)).color(theme::muted(60)));
    ui.add_space(6.0);
}

/// A field that states something rather than taking it — the export dialog's target treatment.
fn read_only(ui: &mut egui::Ui, app: &PryHub, text: &str) {
    let mut owned = text.to_owned();
    ui.add_sized(
        [INNER, app.density.input_height()],
        egui::TextEdit::singleline(&mut owned)
            .interactive(false)
            .font(theme::font::mono(12.0))
            .margin(egui::Margin::symmetric(10, 6))
            .background_color(token::SURFACE),
    );
}

/// The 2 px rule that separates a dialog's regions.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), token::RULE), Sense::hover());
    ui.painter().rect_filled(rect, 0.0_f32, token::DIVIDER);
}
