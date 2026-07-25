//! The frame around the screens: the top bar and the status bar.
//!
//! Everything on these two strips is true of the *session* rather than of a screen — which file is
//! open, which screen is showing, what the worker is doing — so they are drawn once, around
//! whatever the middle is doing, and they live apart from the state they read.

use crate::app::{PryHub, Screen, Tab};
use crate::theme::{self, token};
use egui::{Align, Layout, RichText};

impl PryHub {
    /// Brand · screen nav · open/export · language · density.
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        // Reported by the button, acted on after the bar is drawn — the export needs the whole
        // app, and the bar is holding it while it draws.
        let mut exported = false;
        egui::Panel::top("topbar")
            .exact_size(44.0)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin::symmetric(12, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let brand = ui.add(
                        egui::Label::new(
                            RichText::new("PryHUB").font(theme::font::heading(19.0)).color(token::TEXT),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if brand.clicked() {
                        self.screen = Screen::Welcome;
                    }
                    ui.label(RichText::new(t.brand_sub).size(10.0).color(theme::muted(50)));
                    ui.add_space(theme::token::SPACE_2);

                    let mut active_rect = None;
                    for (screen, label) in [
                        (Screen::Workspace, t.nav_workspace),
                        (Screen::Validation, t.nav_validation),
                        (Screen::Discovery, t.nav_discovery),
                        (Screen::Diff, t.nav_diff),
                        (Screen::Dictionary, t.nav_dict),
                    ] {
                        let resp = nav_button(ui, label, self.screen == screen);
                        if self.screen == screen {
                            active_rect = Some(resp.rect);
                        }
                        if resp.clicked() {
                            self.screen = screen;
                        }
                    }
                    if let Some(rect) = active_rect {
                        slide_underline(ui, egui::Id::new("nav_underline"), rect, 2.0);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(self.density.label()).on_hover_text("yoğunluk / density").clicked() {
                            self.density = self.density.next();
                            self.restyle = true;
                        }
                        if ui.button(self.lang.label()).clicked() {
                            self.lang = self.lang.other();
                        }
                        ui.separator();
                        if ui.button(t.m_open).clicked() {
                            self.screen = Screen::Welcome;
                        }
                        // Enabled only with a file open: a button that can do nothing is worse
                        // than one that is visibly not for now.
                        let can = self.doc.is_some();
                        if ui
                            .add_enabled(can, egui::Button::new(t.m_export))
                            .on_hover_text(t.export_hint)
                            .clicked()
                        {
                            exported = true;
                        }
                    });
                });
            });
        if exported {
            // Which of the three the button means is a question about the interface, so it is
            // answered here rather than inside the job.
            let kind = match self.tab {
                Tab::Texture => crate::export::Kind::Textures,
                _ => crate::export::Kind::Model,
            };
            self.export_now(kind);
        }
    }

    /// File · size · chunk count · selection · codec · scale.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        let busy = self.jobs.busy();
        let progress = self.jobs.progress();
        egui::Panel::bottom("statusbar")
            .exact_size(22.0)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin::symmetric(10, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let small = |ui: &mut egui::Ui, s: String, strong: bool| {
                        let mut txt = RichText::new(s).font(theme::font::mono(10.5));
                        txt = if strong { txt.color(token::TEXT) } else { txt.color(theme::muted(60)) };
                        ui.label(txt);
                    };
                    match &self.doc {
                        Some(doc) => {
                            small(ui, doc.file_name(), true);
                            small(ui, format!("{:.2} MB", doc.bytes.len() as f32 / (1024.0 * 1024.0)), false);
                            small(ui, format!("{} {}", doc.rows.len(), t.st_chunks), false);
                            if let Some(sel) = self.selection.and_then(|o| doc.node_at(o)) {
                                small(
                                    ui,
                                    format!("{} {:#010x} · {} B", t.st_sel, sel.header.id, sel.header.size),
                                    false,
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                small(ui, t.st_scale.to_string(), false);
                                // What the worker is doing, if anything. A word, plus a spinner that
                                // is *moving* — a static "open…" is indistinguishable from a hang.
                                if let Some(job) = busy {
                                    if let Some(share) = progress {
                                        ui.add_sized(
                                            [60.0, 8.0],
                                            egui::ProgressBar::new(share)
                                                .fill(token::ACCENT)
                                                .corner_radius(0),
                                        );
                                    } else {
                                        ui.add(egui::Spinner::new().size(11.0).color(token::ACCENT));
                                    }
                                    ui.label(
                                        RichText::new(format!("· {job}"))
                                            .font(theme::font::mono(10.5))
                                            .color(token::ACCENT),
                                    );
                                }
                                small(ui, format!("{:?}", doc.codec), false);
                            });
                        }
                        None => match busy {
                            Some(job) => small(ui, format!("{job}…"), true),
                            None => small(ui, t.no_file.to_string(), false),
                        },
                    }
                });
            });
    }
}

/// A top-bar navigation button: flat, and the ink of the label fades between states rather than
/// switching. The underline is drawn once, afterwards, so it can *slide* to the active button.
fn nav_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let id = ui.id().with(label);
    let on = ui.ctx().animate_bool_with_time(id, active, MOVE_TIME);
    let color = theme::muted(65).lerp_to_gamma(token::ACCENT, on);
    let text = RichText::new(label).font(theme::font::heading(11.5)).color(color);
    ui.add(egui::Button::new(text).fill(egui::Color32::TRANSPARENT).frame(false))
}

/// How long a moving mark takes to arrive. Long enough to be followed by the eye, short enough that
/// it never delays a click — the point is to show *what moved*, not to perform.
pub(crate) const MOVE_TIME: f32 = 0.12;

/// Slide a 2 px accent underline to `rect`, remembering where it was under `id`.
///
/// Drawn rather than attached to the button: an underline that jumps between two labels reads as two
/// unrelated marks, while one that travels reads as the same mark following the selection.
pub(crate) fn slide_underline(ui: &egui::Ui, id: egui::Id, rect: egui::Rect, y_offset: f32) {
    let ctx = ui.ctx();
    let x = ctx.animate_value_with_time(id.with("x"), rect.left(), MOVE_TIME);
    let w = ctx.animate_value_with_time(id.with("w"), rect.width(), MOVE_TIME);
    ui.painter().hline(
        x..=(x + w),
        rect.bottom() + y_offset,
        egui::Stroke::new(2.0_f32, token::ACCENT),
    );
}

