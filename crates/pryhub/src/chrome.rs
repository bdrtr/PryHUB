//! The frame around the screens: the top bar and the status bar.
//!
//! Everything on these two strips is true of the *session* rather than of a screen — which file is
//! open, which screen is showing, what the worker is doing — so they are drawn once, around
//! whatever the middle is doing, and they live apart from the state they read.

use crate::app::{PryHub, Screen};
use crate::theme::Density;
use crate::theme::{self, token};
use egui::{Align, Layout, RichText};

impl PryHub {
    /// Brand · open/export · screen nav · what the file's health is · settings.
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        // Reported by the button, acted on after the bar is drawn — the export needs the whole
        // app, and the bar is holding it while it draws.
        let mut exported = false;
        // Tallied first: the counter reads the document while the bar is busy writing `self.screen`.
        let health = self.doc.as_ref().map(|doc| doc.health());
        // Copied out for the same reason: the bar writes `self.screen` while it draws.
        let mark = self.logo.as_ref().map(crate::logo::Logo::mark);
        egui::Panel::top("topbar")
            // 44 of content over the design's 2 px rule, which is why the margin ends in a 2 and
            // the panel is 46: the rule is *under* the bar, not a border around it.
            .exact_size(46.0)
            // egui would rule the panel's edge itself, in its own width and its own grey, directly
            // under the one the design asks for. One rule, painted here.
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 0,
                bottom: 2,
            }))
            .show_inside(ui, |ui| {
                let content = ui.max_rect();
                let clip = ui.clip_rect();
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(clip.left(), content.bottom()),
                        egui::pos2(clip.right(), content.bottom() + 2.0),
                    ),
                    0.0_f32,
                    token::DIVIDER_ON_SURFACE,
                );

                ui.horizontal_centered(|ui| {
                    // The design's `gap:10px` between everything on this row.
                    ui.spacing_mut().item_spacing.x = 10.0;

                    // Brand: the word and its subtitle share a baseline and a click.
                    let brand = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            // The mark, set to the wordmark's own cap height rather than to the
                            // bar's: a logo as tall as the strip it sits in reads as a button.
                            if let Some(mark) = mark {
                                mark.show(ui, 22.0);
                            }
                            ui.label(
                                RichText::new("PryHUB").font(theme::font::heading(19.0)).color(token::TEXT),
                            );
                            ui.label(RichText::new(t.brand_sub).size(10.0).color(theme::muted(50)));
                        })
                        .response
                        .interact(egui::Sense::click());
                    if brand.clicked() {
                        self.screen = Screen::Welcome;
                    }
                    crate::widget::rule_v(ui, 22.0, token::DIVIDER_ON_SURFACE);

                    // The two file actions: they act on the *document*, while the nav beyond them
                    // only chooses which way to look at it.
                    if crate::widget::action(ui, t.m_open, true, theme::icon::open).clicked() {
                        self.screen = Screen::Welcome;
                    }
                    // Enabled only with a file open: a button that can do nothing is worse than one
                    // that is visibly not for now.
                    let can = self.doc.is_some();
                    if crate::widget::action(ui, t.m_export, can, theme::icon::export)
                        .on_hover_text(t.export_hint)
                        .clicked()
                    {
                        exported = true;
                    }

                    ui.add_space(theme::token::SPACE_2);
                    crate::widget::segmented(
                        ui,
                        egui::Id::new("nav"),
                        &mut self.screen,
                        &[
                            (Screen::Workspace, t.nav_workspace),
                            (Screen::Validation, t.nav_validation),
                            (Screen::Discovery, t.nav_discovery),
                            (Screen::Diff, t.nav_diff),
                            (Screen::Dictionary, t.nav_dict),
                        ],
                        crate::widget::Seg::Nav,
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        self.settings_menu(ui);
                        if let Some(counts) = health {
                            if health_counter(ui, counts).on_hover_text(t.nav_validation).clicked() {
                                self.screen = Screen::Validation;
                            }
                        }
                    });
                });
            });
        if exported {
            // The button opens the dialog rather than firing: what an export means used to be
            // guessed from which tab happened to be in front, and a file whose contents depend on
            // something the user was not thinking about is not an answer.
            self.show_export = true;
            self.screen = Screen::Workspace;
        }
    }

    /// File · size · chunk count · selection · codec · scale.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        let busy = self.jobs.busy();
        let progress = self.jobs.progress();
        egui::Panel::bottom("statusbar")
            // 24 of content under the design's 2 px rule, the top bar's arrangement mirrored.
            .exact_size(26.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(token::SURFACE).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 2,
                bottom: 0,
            }))
            .show_inside(ui, |ui| {
                let content = ui.max_rect();
                let clip = ui.clip_rect();
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(clip.left(), content.top() - 2.0),
                        egui::pos2(clip.right(), content.top()),
                    ),
                    0.0_f32,
                    token::DIVIDER_ON_SURFACE,
                );
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let small = |ui: &mut egui::Ui, s: String, strong: bool| {
                        let mut txt = RichText::new(s).font(theme::font::mono(10.5));
                        txt = if strong { txt.color(token::TEXT) } else { txt.color(theme::muted(58)) };
                        ui.label(txt);
                    };
                    match &self.doc {
                        Some(doc) => {
                            small(ui, doc.file_name(), true);
                            small(ui, format!("{:.2} MB", doc.bytes.len() as f32 / (1024.0 * 1024.0)), false);
                            small(ui, format!("{} {}", doc.rows.len(), t.st_chunks.of(doc.rows.len())), false);
                            if let Some(sel) = self.selection.and_then(|o| doc.node_at(o)) {
                                // The design's `|`, in the divider's own ink: it separates what is
                                // true of the file from what is true of the selection.
                                ui.label(
                                    RichText::new("|")
                                        .font(theme::font::mono(10.5))
                                        .color(token::DIVIDER_ON_SURFACE),
                                );
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
                                        RichText::new(format!("· {}", job_word(job, t)))
                                            .font(theme::font::mono(10.5))
                                            .color(token::ACCENT),
                                    );
                                }
                                // The design's `JDLZ ✓`: the codec in the file's own spelling, and
                                // a tick that says it came back out. `None` has nothing to tick.
                                use gizmo_nfs::compression::Codec;
                                if !matches!(doc.codec, Codec::None) {
                                    let (rect, _) =
                                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                    theme::mark::ok(ui.painter(), rect.center(), 10.0, theme::muted(58));
                                }
                                let name = match doc.codec {
                                    Codec::RefPack => "REFPACK".to_string(),
                                    Codec::Jdlz => "JDLZ".to_string(),
                                    Codec::Huff => "HUFF".to_string(),
                                    Codec::None => "RAW".to_string(),
                                    // The parser is free to learn a codec this list has not heard
                                    // of; its own name for it is better than a blank.
                                    other => format!("{other:?}").to_uppercase(),
                                };
                                small(ui, name, false);
                            });
                        }
                        None => match busy {
                            Some(job) => small(ui, format!("{}…", job_word(job, t)), true),
                            None => small(ui, t.no_file.to_string(), false),
                        },
                    }
                });
            });
    }
}

/// What the worker is doing, in the interface's language.
///
/// `jobs::Kind` is deliberately a kind rather than a word: the program's log wants one language and
/// the status bar wants the user's, and this is where the second is answered.
fn job_word(kind: crate::jobs::Kind, t: &crate::i18n::Strings) -> &'static str {
    match kind {
        crate::jobs::Kind::Open => t.job_open,
        crate::jobs::Kind::Decode => t.job_decode,
        crate::jobs::Kind::Export => t.job_export,
        crate::jobs::Kind::Palette => t.job_palette,
    }
}

/// The design's health pill: `⚠ n` beside `✓ n`, in a hairline box that leads to the screen the
/// numbers came from.
///
/// The warning mark pulses while there is anything to warn about. It is the one moving thing in a
/// still bar, so it is also the one thing that costs a repaint — which is why it only animates when
/// the count is not zero, and stands still on a clean file.
fn health_counter(ui: &mut egui::Ui, (warn, ok): (usize, usize)) -> egui::Response {
    // Measured and painted by hand rather than nested in layouts: this is built inside the bar's
    // right-to-left run, where a `horizontal` reads its children backwards and a stated
    // left-to-right one claims the whole remaining width. Three numbers on one line are not worth
    // fighting a layout for.
    const MARK: f32 = 12.0;
    let font = theme::font::heading(12.0);
    let warn_n = ui.painter().layout_no_wrap(warn.to_string(), font.clone(), token::ACCENT_700);
    let ok_n = ui.painter().layout_no_wrap(ok.to_string(), font, token::NEUTRAL_700);

    // The design's box: 4 × 9 padding, 9 between the three groups, 4 inside each.
    let size = egui::vec2(
        9.0 + MARK + 4.0 + warn_n.size().x + 9.0 + 2.0 + 9.0 + MARK + 4.0 + ok_n.size().x + 9.0,
        8.0 + warn_n.size().y.max(MARK),
    );
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter();
    p.rect_stroke(
        rect,
        0.0_f32,
        egui::Stroke::new(1.0_f32, token::DIVIDER_ON_SURFACE),
        egui::StrokeKind::Inside,
    );

    // `warnpulse`: opacity 1 → .35 → 1 over 1.8 s, eased at both ends — which is a cosine, so it is
    // written as one rather than approximated with keyframes.
    let alpha = if warn > 0 {
        ui.ctx().request_repaint();
        let phase = ui.input(|i| i.time as f32) * std::f32::consts::TAU / 1.8;
        0.35 + 0.65 * (phase.cos() * 0.5 + 0.5)
    } else {
        1.0
    };

    // Both marks are drawn rather than typed: neither `⚠` nor `✓` survives the heading face, and a
    // tofu box beside a number would read as the failure itself.
    let y = rect.center().y;
    let mut x = rect.left() + 9.0;
    theme::mark::warn(p, egui::pos2(x + MARK * 0.5, y), MARK, token::ACCENT_700.gamma_multiply(alpha));
    x += MARK + 4.0;
    p.galley(egui::pos2(x, y - warn_n.size().y * 0.5), warn_n.clone(), token::ACCENT_700);
    x += warn_n.size().x + 9.0;
    p.rect_filled(
        egui::Rect::from_min_size(egui::pos2(x, y - 6.0), egui::vec2(2.0, 12.0)),
        0.0_f32,
        token::DIVIDER_ON_SURFACE,
    );
    x += 2.0 + 9.0;
    theme::mark::ok(p, egui::pos2(x + MARK * 0.5, y), MARK, token::NEUTRAL_700);
    x += MARK + 4.0;
    p.galley(egui::pos2(x, y - ok_n.size().y * 0.5), ok_n, token::NEUTRAL_700);

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl PryHub {
    /// The settings menu: the two things the design had bare switches for, plus where to read about
    /// the tool.
    ///
    /// A menu rather than two unlabelled buttons in the corner. `TR` and `S` were legible to whoever
    /// wrote them and to nobody else — and a setting that cannot be found is not a setting. What
    /// belongs here is what is true of the whole program rather than of a file: its language, its
    /// size, and how to reach its source.
    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        let t = self.lang.strings();
        let before = crate::settings::Settings { lang: self.lang, density: self.density };
        // Set like the design's segment groups it stands beside — this is where its two switches
        // went, so it should look like the boxes that used to hold them.
        ui.menu_button(RichText::new(t.m_settings).font(theme::font::heading(11.0)), |ui| {
            ui.set_min_width(190.0);

            ui.label(RichText::new(t.set_language).size(10.5).color(theme::muted(55)));
            for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Tr] {
                if ui.selectable_label(self.lang == lang, lang.name()).clicked() {
                    self.lang = lang;
                }
            }

            ui.add_space(theme::token::SPACE_2);
            ui.label(RichText::new(t.set_size).size(10.5).color(theme::muted(55)));
            for density in Density::all() {
                // Named rather than lettered: "M" is only obvious once you know it is not "menu".
                let name = match density {
                    Density::Compact => t.set_small,
                    Density::Balanced => t.set_medium,
                    Density::Roomy => t.set_large,
                };
                if ui.selectable_label(self.density == density, name).clicked() {
                    self.density = density;
                    self.restyle = true;
                }
            }

            ui.add_space(theme::token::SPACE_2);
            ui.label(RichText::new(t.set_about).size(10.5).color(theme::muted(55)));
            // `open_url` goes through egui's own output command, so the platform's browser opens it
            // and this crate needs nothing to do it.
            if ui.selectable_label(false, t.set_help).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(HELP_URL));
            }
            if ui.selectable_label(false, t.set_repo).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(REPO_URL));
            }
            ui.add_space(theme::token::SPACE_1);
            ui.label(
                RichText::new(format!("PryHUB {}", env!("CARGO_PKG_VERSION")))
                    .font(theme::font::mono(10.0))
                    .color(theme::muted(45)),
            );
        });
        // Written when it changes, so the next window opens the way this one was left.
        let after = crate::settings::Settings { lang: self.lang, density: self.density };
        if after != before {
            after.save();
        }
    }
}

/// Where the menu's two links go. The README is the help, because it is the document that is kept up
/// to date by being the one people read first.
const REPO_URL: &str = "https://github.com/bdrtr/PryHUB";
const HELP_URL: &str = "https://github.com/bdrtr/PryHUB#readme";

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


