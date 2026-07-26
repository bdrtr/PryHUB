//! The save dialog: what is about to go into the pack, and where the pack lands.
//!
//! It used to be a *replace* dialog — one texture, one image, write it now — and that was the wrong
//! shape for what it does. A write reads the pack from disk, so two replacements done one at a time
//! into a copy each started from the original and the second discarded the first. Staging fixes that
//! at the root, and staging needs somewhere to say what is staged.
//!
//! **The design already has that vocabulary and it is not on this screen.** Its CARP screen edits
//! into a pending set, puts an accent dot on what is dirty, counts it in a chip, lists the changes
//! from → to, and closes with a Revert and a Save. This borrows all of that — including `cp_revert`,
//! which is CARP's own string and says only "Revert". Its Save is *not* borrowed: `cp_save` reads
//! "Save CARP", which named a screen this button has nothing to do with, and it said so on screen
//! until it was noticed. Borrow a treatment, not a sentence about somewhere else.
//!
//! The dirty dot is **drawn** rather than typed, for the reason the validation marks and the
//! assembly tab's triangle count are: the bundled font has no `●`, and a missing glyph is a box. It
//! was a box here first.
//!
//! The one question the export dialog never has to ask stays here: an export writes a folder nothing
//! else reads, and this writes a `TEXTURES.BIN`, which the *game* reads. A copy is the default and
//! the original is a deliberate second choice, with the resolved path on screen before the button is
//! pressed rather than in the log after it.

use crate::app::PryHub;
use crate::theme::{self, token};
use egui::{Color32, RichText, Sense, Vec2};

/// The export dialog's measures, which are the design's for a modal — one object at every density.
const WIDTH: f32 = 480.0;
const PAD_X: f32 = 18.0;
const INNER: f32 = WIDTH - PAD_X * 2.0;

/// How many staged rows are listed before the strip says "and n more".
///
/// The design's own pending strip is a bounded scroller (`max-height: 118px`); this is the same
/// bound expressed in rows, because a dialog that grows with the set stops being a dialog.
const LISTED: usize = 6;

/// Draw the dialog if it is open, and act on what was pressed.
pub fn show(app: &mut PryHub, ctx: &egui::Context) {
    if !app.show_replace {
        return;
    }
    let t = app.lang.strings();
    // Reached by pressing Save there is always a document, a pack and a staged set; reached by
    // `--screen replace` the file has not even been opened yet, and opening and decoding are both
    // jobs. So a missing piece is only a reason to give up when nothing is coming.
    let waiting = app.jobs.busy().is_some()
        || matches!(app.textures, crate::app::Textures::Unasked | crate::app::Textures::Decoding);
    let Some(doc) = app.doc.clone() else {
        if !waiting {
            app.show_replace = false;
        }
        return;
    };
    let Some(pack) = doc.pack_path() else {
        if !waiting {
            app.show_replace = false;
        }
        return;
    };
    if app.pending.is_empty() {
        // Nothing staged is not an error state to sit in — there is nothing for this dialog to be
        // about. (`--screen replace` on a fresh file lands here, which is why it waits first.)
        if !waiting {
            app.show_replace = false;
        }
        return;
    }

    let mut over = app.replace_over;
    let mut close = false;
    let mut go = false;
    let mut revert = false;

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
            body(ui, app, t, &pack, &mut over);
            footer(ui, t, app.pending.len(), &mut close, &mut go, &mut revert);
        });

    app.replace_over = over;
    if response.should_close() {
        close = true;
    }
    if revert {
        app.pending.clear();
        close = true;
    }
    if go {
        app.replace_now();
        close = true;
    }
    if close {
        app.show_replace = false;
        // The overwrite flag does not outlive the dialog — see the field's own note.
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

/// What is staged, and where the pack goes.
fn body(
    ui: &mut egui::Ui,
    app: &PryHub,
    t: &'static crate::i18n::Strings,
    pack: &std::path::Path,
    over: &mut bool,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 16, bottom: 16 })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            group_label(ui, t.rep_list);
            // One row per staged texture: the accent dot the design puts on a dirty row, the
            // texture's name, and the file going into it.
            for p in app.pending.iter().take(LISTED) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    theme::mark::dot(ui.painter(), dot.center(), 7.0, token::ACCENT);
                    ui.label(
                        RichText::new(&p.name).font(theme::font::mono(11.5)).color(token::TEXT),
                    );
                    ui.label(RichText::new("→").size(11.0).color(theme::muted(45)));
                    let file = p.png.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ui.label(
                        RichText::new(file)
                            .font(theme::font::mono(11.5))
                            .color(token::ACCENT.gamma_multiply(0.9)),
                    );
                });
                ui.add_space(3.0);
            }
            if app.pending.len() > LISTED {
                ui.label(
                    RichText::new(format!("+{}", app.pending.len() - LISTED))
                        .font(theme::font::mono(11.0))
                        .color(theme::muted(45)),
                );
                ui.add_space(3.0);
            }
            ui.add_space(3.0);
            ui.label(RichText::new(t.rep_note).font(theme::font::body(10.5)).color(theme::muted(52)));
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
            let mut owned = path;
            ui.add_sized(
                [INNER, app.density.input_height()],
                egui::TextEdit::singleline(&mut owned)
                    .interactive(false)
                    .font(theme::font::mono(12.0))
                    .margin(egui::Margin::symmetric(10, 6))
                    .background_color(token::SURFACE),
            );
        });
}

/// Revert, Cancel and the primary, over the 2 px rule that closes the dialog.
fn footer(
    ui: &mut egui::Ui,
    t: &'static crate::i18n::Strings,
    staged: usize,
    close: &mut bool,
    go: &mut bool,
    revert: &mut bool,
) {
    rule(ui);
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = token::SPACE_2;
                // The label counts what it is about to write, the way the export's counts the build.
                let label = format!("{} · {staged} {}", t.rep_go, t.textures_count.of(staged));
                if crate::widget::button_primary(ui, &label).clicked() {
                    *go = true;
                }
                if crate::widget::button_secondary(ui, t.exp_cancel).clicked() {
                    *close = true;
                }
                // CARP's own word, and its own place: throwing the set away sits beside the button
                // that writes it, not somewhere else.
                if crate::widget::button_secondary(ui, t.cp_revert).clicked() {
                    *revert = true;
                }
            });
        });
}

/// The design's 11 px / 60 % label over a row.
fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font::body(11.0)).color(theme::muted(60)));
    ui.add_space(6.0);
}

/// The 2 px rule that separates a dialog's regions.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), token::RULE), Sense::hover());
    ui.painter().rect_filled(rect, 0.0_f32, token::DIVIDER);
}
