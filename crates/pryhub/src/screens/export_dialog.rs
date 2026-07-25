//! The export dialog: what to write, in what, and where it lands.
//!
//! The button in the top bar used to fire an export on the spot and decide what it meant by looking
//! at which tab was in front. That is a guess dressed as a shortcut — the file that comes out
//! depends on something the user was not thinking about. Here the two questions are asked out loud,
//! and the primary button says what it is about to do before it does it.
//!
//! Only the choices the tool can honestly keep are offered. There is no DDS writer anywhere in
//! `gizmo-nfs`, so the texture row is drawn — the design's three-row rhythm is what makes the body
//! read — with its two impossible options in the design's own disabled treatment rather than
//! silently dropped or, worse, silently ignored.

use crate::app::PryHub;
use crate::export::{Choice, Model, Scope};
use crate::theme::{self, token};
use egui::{Color32, RichText, Sense, Stroke, Vec2};

/// The design's fixed measures. A dialog is one object at every density — the `D` table the tree
/// and the hex view consult is never asked about this one.
const WIDTH: f32 = 480.0;
const PAD_X: f32 = 18.0;
const INNER: f32 = WIDTH - PAD_X * 2.0;

/// Draw the dialog if it is open, and act on what was pressed.
pub fn show(app: &mut PryHub, ctx: &egui::Context) {
    if !app.show_export {
        return;
    }
    let t = app.lang.strings();
    let Some(doc) = app.doc.clone() else {
        // Opening is a job, so `--screen export` raises this flag a frame or two before there is a
        // document to export. Clearing it here consumed the request before the parse it was waiting
        // for — the same trap `pending_selection` exists to avoid, and it was a race rather than a
        // reliable failure, which is worse. The dialog waits while the worker is still busy and
        // only gives up when nothing is coming.
        if app.jobs.busy().is_none() {
            app.show_export = false;
        }
        return;
    };
    // What the file can actually give. A TPK holds only images, so the model row would be a promise
    // about a mesh that is not in there.
    let has_parts = !doc.parts.is_empty();
    let mut choice = app.export_choice;
    let mut close = false;
    let mut go = false;

    let response = egui::Modal::new(egui::Id::new("export_dialog"))
        .backdrop_color(token::SCRIM)
        .frame(egui::Frame::new().fill(token::SURFACE).shadow(theme::shadow::LG))
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // `border-top: 3px solid var(--color-accent)` — one edge, which `egui::Frame`'s single
            // uniform stroke cannot express.
            let top = ui.cursor().top();
            let x = ui.max_rect().x_range();
            ui.painter().rect_filled(
                egui::Rect::from_x_y_ranges(x, top..=top + 3.0),
                0.0_f32,
                token::ACCENT,
            );
            ui.add_space(3.0);

            header(ui, t.exp_title, &mut close);
            body(ui, app, &doc, t, has_parts, &mut choice);
            footer(ui, app, t, choice, has_parts, &mut close, &mut go);
        });

    app.export_choice = choice;
    // Escape, and a click on the dimmed backdrop — the design's own two silent ways out.
    if response.should_close() {
        close = true;
    }
    if go {
        // A file with no parts has only its textures to give, and the dialog has already said so by
        // disabling the model row; asking for them by name is clearer than relying on the fallback
        // inside the job.
        let kind = if has_parts { crate::export::Kind::Model } else { crate::export::Kind::Textures };
        app.export_now(kind);
        close = true;
    }
    if close {
        app.show_export = false;
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
                    // `×` is Latin-1 and Archivo has it — this is the one mark on the dialog that
                    // does not need drawing.
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

/// The three option rows and the target folder.
fn body(
    ui: &mut egui::Ui,
    app: &PryHub,
    doc: &crate::doc::Doc,
    t: &'static crate::i18n::Strings,
    has_parts: bool,
    choice: &mut Choice,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 16, bottom: 16 })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            group_label(ui, t.exp_scope);
            crate::widget::segmented_filling(
                ui,
                egui::Id::new("exp_scope"),
                &mut choice.scope,
                &[
                    (Scope::Selection, t.exp_scope_selection),
                    (Scope::Build, t.exp_scope_build),
                    (Scope::All, t.exp_scope_all),
                ],
                INNER,
            );
            ui.add_space(token::SPACE_4);

            group_label(ui, t.exp_model);
            if has_parts {
                crate::widget::segmented_filling(
                    ui,
                    egui::Id::new("exp_model"),
                    &mut choice.model,
                    &[(Model::Gltf, "glTF"), (Model::Obj, "OBJ"), (Model::Both, t.exp_both)],
                    INNER,
                );
            } else {
                dead_row(ui, &["glTF", "OBJ", t.exp_both], None, t.no_parts);
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(t.exp_note).font(theme::font::body(10.5)).color(theme::muted(52)),
            );
            ui.add_space(token::SPACE_4);

            group_label(ui, t.exp_tex);
            // PNG is the only decoded output the parser has; DDS would be a promise nothing behind
            // this dialog can keep.
            dead_row(ui, &["PNG", "DDS", t.exp_both], Some(0), t.exp_png_only);
            ui.add_space(token::SPACE_4);

            // `.field > label` is deliberately not the group label above it: 12 px at 70 %, not
            // 11 px at 60 %.
            ui.label(RichText::new(t.exp_target).font(theme::font::body(12.0)).color(theme::muted(70)));
            ui.add_space(5.0);
            let mut path = crate::export::out_dir(doc)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|e| e);
            ui.add_sized(
                [INNER, app.density.input_height()],
                egui::TextEdit::singleline(&mut path)
                    .interactive(false)
                    .font(theme::font::mono(12.0))
                    .margin(egui::Margin::symmetric(10, 6))
                    .background_color(token::SURFACE),
            );
        });
}

/// Cancel and the primary, over the 2 px rule that closes the dialog.
fn footer(
    ui: &mut egui::Ui,
    app: &PryHub,
    t: &'static crate::i18n::Strings,
    choice: Choice,
    has_parts: bool,
    close: &mut bool,
    go: &mut bool,
) {
    rule(ui);
    egui::Frame::new()
        .inner_margin(egui::Margin { left: 18, right: 18, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = token::SPACE_2;
                // The label states what is about to happen and is rebuilt every frame, so it
                // follows the segments as they are clicked.
                let what = if has_parts { choice.model.label() } else { "PNG" };
                let scope = match choice.scope {
                    Scope::All => t.exp_scope_all.to_string(),
                    // The build says how much of it is on, because that is the one thing about it
                    // the button cannot show: `Build · 22/26`.
                    Scope::Build => match &app.doc {
                        Some(doc) => {
                            let rows = crate::panels::assembly::rows(&doc.parts);
                            let on =
                                rows.iter().filter(|r| !app.unmounted.contains(&r.key)).count();
                            format!("{} {on}/{}", t.exp_scope_build, rows.len())
                        }
                        None => t.exp_scope_build.to_string(),
                    },
                    Scope::Selection => app
                        .selected_solid_name()
                        .unwrap_or_else(|| t.exp_scope_selection.to_string()),
                };
                if crate::widget::button_primary(ui, &format!("{} · {what} · {scope}", t.exp_go))
                    .clicked()
                {
                    *go = true;
                }
                if crate::widget::button_secondary(ui, t.exp_cancel).clicked() {
                    *close = true;
                }
            });
        });
}

/// The design's 11 px / 60 % label over an option row.
fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font::body(11.0)).color(theme::muted(60)));
    ui.add_space(6.0);
}

/// An option row nothing can be chosen from: the design's shape, in `.btn:disabled`'s opacity, with
/// a hover that says why. `lit` is the option that is nevertheless true.
fn dead_row(ui: &mut egui::Ui, labels: &[&str], lit: Option<usize>, why: &str) {
    let each = (INNER - 2.0) / labels.len() as f32;
    let resp = egui::Frame::new()
        .stroke(Stroke::new(token::HAIRLINE, token::DIVIDER_ON_SURFACE))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let on = lit == Some(i);
                    let galley = ui.painter().layout_no_wrap(
                        (*label).to_owned(),
                        theme::font::heading(11.0),
                        Color32::PLACEHOLDER,
                    );
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(each, galley.size().y + 10.0),
                        Sense::hover(),
                    );
                    let p = ui.painter();
                    if on {
                        p.rect_filled(rect, 0.0_f32, token::ACCENT.gamma_multiply(0.45));
                    }
                    p.galley(
                        rect.center() - galley.size() * 0.5,
                        galley,
                        if on { token::BG } else { theme::muted(36) },
                    );
                }
            });
        })
        .response;
    resp.on_hover_text(why);
}

/// The 2 px rule that ends the header and opens the footer, in the twin that belongs on a surface.
fn rule(ui: &mut egui::Ui) {
    let x = ui.max_rect().x_range();
    let y = ui.cursor().top();
    ui.painter().rect_filled(
        egui::Rect::from_x_y_ranges(x, y..=y + token::RULE),
        0.0_f32,
        token::DIVIDER_ON_SURFACE,
    );
    ui.add_space(token::RULE);
}
