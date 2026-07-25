//! The validation screen: one card per rule, with what it found and what it read.
//!
//! The rules are the parser's ([`gizmo_nfs::validate`]) and run once when the file opens; this
//! screen is a read of that report. The count of chunks each rule examined is shown next to its
//! findings on purpose — a rule with nothing to report because it read nothing is not a pass, and
//! the difference is the whole reason to trust the other four.

use crate::app::PryHub;
use crate::theme::{self, token};
use crate::widget;
use egui::RichText;
use gizmo_nfs::validate::{RuleResult, Severity, RULES};

/// The design's `max-width:880px; padding:34px 34px 40px` — the column the whole screen is set
/// in. Border-box, so the content is 880 − 68 = 812; [`widget::page`] does that subtraction.
const PAGE_WIDTH: f32 = 880.0;
const PAGE_PAD: egui::Margin = egui::Margin { left: 34, right: 34, top: 34, bottom: 40 };
/// `gap: 14px` between two cards, `gap: 10px` inside a card's own rows.
const CARD_GAP: f32 = 14.0;
const ROW_GAP: f32 = 10.0;
/// The line box a check row is given: the design's `12.5px × 1.55`. Left alone, egui makes every
/// horizontal row at least one *control* tall, which is 5 px more than a row of text is worth here.
const LINE: f32 = 12.5 * 1.55;

/// Draw the screen; returns a chunk offset if a finding was clicked.
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) -> Option<usize> {
    let t = app.lang.strings();
    let mut select = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(token::BG))
        .show_inside(ui, |ui| {
            // The design scrolls the column whole, header included — the title is part of the page,
            // not a bar pinned over it.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                widget::page(ui, PAGE_WIDTH, PAGE_PAD, |ui| {
                    header(app, ui);

                    let Some(doc) = &app.doc else {
                        ui.label(
                            RichText::new(t.no_file)
                                .font(theme::font::body(12.5))
                                .color(theme::muted(50)),
                        );
                        return;
                    };
                    ui.spacing_mut().item_spacing.y = CARD_GAP;
                    if doc.report.worst().is_none() {
                        ui.label(
                            RichText::new(t.val_no_findings)
                                .font(theme::font::body(12.5))
                                .color(token::NEUTRAL_600),
                        );
                    }
                    for rule in RULES {
                        let Some(result) = doc.report.results.iter().find(|r| r.rule == rule.id)
                        else {
                            continue;
                        };
                        if let Some(offset) = card(app, ui, rule, result) {
                            select = Some(offset);
                        }
                    }
                });
            });
        });
    select
}

/// The screen's header: the title, what was read, and the two totals.
///
/// Laid out here rather than through [`widget::screen_header`] because this is the one screen whose
/// header carries a right-hand slot, and that primitive has nowhere to put one.
fn header(app: &PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = token::SPACE_4;
        ui.label(RichText::new(t.nav_validation).font(theme::font::h2()));
        if let Some(doc) = &app.doc {
            ui.label(
                RichText::new(format!("{} · {}", doc.file_name(), t.v_auto))
                    .font(theme::font::mono(12.0))
                    .color(theme::muted(55)),
            );
            // What the two chips count — the same tally the top bar's pill draws, taken from the
            // one place that counts it, so the number a click leads away from is the number it
            // leads to. A rule that read nothing is neither a warning nor a pass, so it is in
            // neither total; the card below says so itself.
            let (warns, passed) = doc.health();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = token::SPACE_2;
                // Right to left, so the design's order — warnings, then passes — is drawn backwards.
                // Each carries its mark, the way the design writes them: `⚠ 2 warnings`, `✓ 5 passed`.
                widget::tag_marked(
                    ui,
                    &format!("{passed} {}", t.v_pass),
                    widget::Tone::Neutral,
                    theme::mark::ok,
                );
                widget::tag_marked(
                    ui,
                    &format!("{warns} {}", t.v_warns.of(warns)),
                    widget::Tone::Accent,
                    theme::mark::warn,
                );
            });
        }
    });
    ui.add_space(token::SPACE_4);
    let x = ui.max_rect().x_range();
    theme::draw::rule_h(ui.painter(), x.min..=x.max, ui.cursor().top(), token::RULE, token::DIVIDER);
    ui.add_space(token::RULE + 22.0);
}

/// One rule's card: title, the expression it applies, what it read, and its findings.
fn card(
    app: &PryHub,
    ui: &mut egui::Ui,
    rule: &gizmo_nfs::validate::Rule,
    result: &RuleResult,
) -> Option<usize> {
    let t = app.lang.strings();
    let mut select = None;
    egui::Frame::new()
        .fill(token::SURFACE)
        .stroke(egui::Stroke::new(token::HAIRLINE, token::DIVIDER_ON_SURFACE))
        // The card has no padding of its own: its header and its rows carry theirs, which is what
        // lets a rule run the full width between them.
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            // …and which only holds if the body is laid out to the column rather than to its own
            // longest row.
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().interact_size.y = LINE;
            let span = ui.max_rect().shrink(token::HAIRLINE).x_range();

            egui::Frame::new()
                .inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 10 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = ROW_GAP;
                        // Drawn, not typed: a missing glyph would render as a box on the one mark
                        // that has to be unmistakable.
                        let (rect, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::hover());
                        let (p, at) = (ui.painter(), rect.center());
                        if result.examined == 0 {
                            theme::mark::unchecked(p, at, 15.0);
                        } else {
                            status_mark(p, at, 15.0, result.status());
                        }
                        ui.label(RichText::new(rule.title).font(theme::font::heading(14.0)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(rule.expr)
                                    .font(theme::font::mono(11.0))
                                    .color(theme::muted(50)),
                            );
                        });
                    });
                });
            theme::draw::rule_h(
                ui.painter(),
                span.min..=span.max,
                ui.cursor().top(),
                token::HAIRLINE,
                token::DIVIDER_ON_SURFACE,
            );
            ui.add_space(token::HAIRLINE);

            if result.examined == 0 {
                // Said plainly rather than dressed as a pass.
                check_row(
                    ui,
                    span,
                    |p, at| theme::mark::unchecked(p, at, 13.0),
                    RichText::new(t.val_unread)
                        .font(theme::font::body(12.5))
                        .color(theme::muted(55)),
                    None,
                );
                return;
            }
            // The design lists one row per check, passes included; `RuleResult` carries only the
            // count of what it read, so that becomes one row instead of many. A ring rather than a
            // tick: this row is a tally, not a verdict.
            check_row(
                ui,
                span,
                |p, at| theme::mark::unchecked(p, at, 13.0),
                RichText::new(format!(
                    "{} {} · {} {}",
                    result.examined,
                    t.val_examined.of(result.examined),
                    result.findings.len(),
                    t.val_findings.of(result.findings.len())
                ))
                .font(theme::font::body(12.5))
                .color(theme::muted(55)),
                None,
            );

            for f in &result.findings {
                let resp = check_row(
                    ui,
                    span,
                    |p, at| status_mark(p, at, 13.0, Some(f.severity)),
                    RichText::new(format!("{}: {}", f.subject, f.message))
                        .font(theme::font::body(12.5)),
                    Some(format!("{:#010x}", f.chunk_id)),
                );
                if resp.clicked() {
                    select = Some(f.chunk_offset);
                }
            }
        });
    select
}

/// One row of a card: `padding: 8px 14px`, a mark leading, the detail taking the slack, the chunk
/// id pinned to the right edge, and the half-strength rule that closes it.
///
/// `span` is the card's own width — a row's rule runs the whole card, not just the text.
fn check_row(
    ui: &mut egui::Ui,
    span: egui::Rangef,
    mark: impl FnOnce(&egui::Painter, egui::Pos2),
    detail: RichText,
    id: Option<String>,
) -> egui::Response {
    let resp = egui::Frame::new()
        .inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 8 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = ROW_GAP;
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                mark(ui.painter(), rect.center());
                // The id is placed first so it pins to the right edge; the detail then takes what
                // is left, and is cut rather than wrapped — the design gives a row one line.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(id) = id {
                        ui.label(
                            RichText::new(id)
                                .font(theme::font::mono(10.5))
                                .color(theme::muted(42)),
                        );
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(detail).truncate());
                    });
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    // `divider 50%` over the surface, which is what the ink at 20 % comes out as.
    theme::draw::rule_h(
        ui.painter(),
        span.min..=span.max,
        ui.cursor().top(),
        token::HAIRLINE,
        theme::muted(20),
    );
    ui.add_space(token::HAIRLINE);
    resp
}

/// The design's `iconFor`, stated once: the ink a status is drawn in. Warn is the accent itself and
/// error the darker step above it — the pairing is easy to get backwards, which is why it lives
/// here rather than at each of the two call sites.
fn status_ink(status: Option<Severity>) -> egui::Color32 {
    match status {
        Some(Severity::Error) => token::ACCENT_700,
        Some(Severity::Warn) => token::ACCENT,
        Some(_) => token::NEUTRAL_600,
        None => token::NEUTRAL_500,
    }
}

/// …and the mark that goes with it.
fn status_mark(p: &egui::Painter, at: egui::Pos2, size: f32, status: Option<Severity>) {
    let ink = status_ink(status);
    match status {
        Some(Severity::Error) => theme::mark::error(p, at, size, ink),
        Some(Severity::Warn) => theme::mark::warn(p, at, size, ink),
        // The design's `ℹ` has no drawn twin, and a tick would claim more than an info line says.
        Some(_) => theme::draw::dot(p, at, size * 0.45, ink),
        None => theme::mark::ok(p, at, size, ink),
    }
}
