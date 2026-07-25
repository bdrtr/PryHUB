//! The design's own controls, as egui widgets.
//!
//! [`crate::theme`] carries the design's *values* — colours, sizes, spacing. This carries the two
//! or three *shapes* it builds out of them and then reuses everywhere: the segmented control, the
//! icon-and-label action button, the vertical rule. They live together because the design does not
//! define them once per screen either; the nav strip, the log filter, the density switch and the
//! export dialog are all the same control at two sizes, and a screen that hand-rolled its own would
//! drift from the rest by a pixel or a shade.

use crate::chrome::MOVE_TIME;
use crate::theme::{self, token};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Which of the design's two segment sizes to draw.
///
/// The design has exactly two — `navBtn` for the screen strip and `segBtn` for everything smaller —
/// and they differ in more than one number, so they are named rather than parameterised.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    /// `navBtn`: 6 × 12 padding, 12 px heading, full-strength ink when inactive.
    Nav,
    /// `segBtn`: 5 × 11 padding, 11 px heading, 80 % ink when inactive.
    Small,
}

impl Seg {
    /// Padding, as the design writes it: vertical then horizontal.
    fn pad(self) -> Vec2 {
        match self {
            Self::Nav => Vec2::new(12.0, 6.0),
            Self::Small => Vec2::new(11.0, 5.0),
        }
    }

    fn font(self) -> egui::FontId {
        match self {
            Self::Nav => theme::font::heading(12.0),
            Self::Small => theme::font::heading(11.0),
        }
    }

    /// The ink of a segment that is *not* the selected one.
    fn idle_ink(self) -> Color32 {
        match self {
            Self::Nav => token::TEXT,
            Self::Small => theme::muted(80),
        }
    }
}

/// The design's segmented control: touching buttons inside one hairline box, the selected one
/// filled with the accent and its label knocked out in the page colour.
///
/// The accent block is a single shape that *slides* to the selection rather than one fill per
/// button appearing and disappearing — the same reasoning as everywhere else in this interface: one
/// mark that moves reads as the selection following the click, five that blink read as five marks.
///
/// Returns whether the selection changed.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: egui::Id,
    current: &mut T,
    items: &[(T, &str)],
    seg: Seg,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .stroke(Stroke::new(1.0_f32, token::DIVIDER))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                // Reserved before any label is drawn, so the fill lands *behind* them when it is
                // finally known where it goes.
                let slot = ui.painter().add(egui::Shape::Noop);
                let mut active_rect = None;
                for (value, label) in items {
                    let active = *value == *current;
                    let resp = segment(ui, label, active, seg);
                    if active {
                        active_rect = Some(resp.rect);
                    }
                    if resp.clicked() {
                        *current = *value;
                        changed = true;
                    }
                }
                if let Some(r) = active_rect {
                    let ctx = ui.ctx();
                    let x = ctx.animate_value_with_time(id.with("x"), r.left(), MOVE_TIME);
                    let w = ctx.animate_value_with_time(id.with("w"), r.width(), MOVE_TIME);
                    let rect = Rect::from_min_size(Pos2::new(x, r.top()), Vec2::new(w, r.height()));
                    ui.painter().set(slot, egui::Shape::rect_filled(rect, 0.0_f32, token::ACCENT));
                }
            });
        });
    changed
}

/// One segment. Draws only its label — the selected one's fill belongs to the group, which is the
/// only place that knows where the block is travelling from.
fn segment(ui: &mut Ui, label: &str, active: bool, seg: Seg) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), seg.font(), Color32::PLACEHOLDER);
    let pad = seg.pad();
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());

    // The label crosses to the page colour as the block arrives under it, over the same time — one
    // motion, not a label that switches while the fill is still on its way.
    let on = ui.ctx().animate_bool_with_time(resp.id, active, MOVE_TIME);
    let ink = seg.idle_ink().lerp_to_gamma(token::BG, on);
    // The design gives a bare segment no hover at all; its own bordered buttons wash 7 % of the ink
    // over themselves, and a desktop control that answers the pointer with nothing feels broken.
    if !active && resp.hovered() {
        ui.painter().rect_filled(rect, 0.0_f32, theme::muted(7));
    }
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// An icon-and-label action button — the design's `.btn` with an SVG in front of the text.
///
/// Laid out by hand because the glyph is *drawn* ([`theme::icon`]) and [`egui::Button`] has nowhere
/// to put a painted mark. `enabled` dims it to the design's disabled opacity instead of removing
/// it, and it keeps its hover so the tooltip can still say what it would do once there is a file.
pub fn action(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    icon: fn(&egui::Painter, Pos2, f32, Color32),
) -> Response {
    // The design's numbers: a 15 px icon, a 6 px gap, 5 × 9 padding, the heading face at 12 px.
    const ICON: f32 = 15.0;
    const GAP: f32 = 6.0;
    let pad = Vec2::new(9.0, 5.0);

    let galley =
        ui.painter().layout_no_wrap(label.to_owned(), theme::font::heading(12.0), Color32::PLACEHOLDER);
    let size = Vec2::new(
        pad.x * 2.0 + ICON + GAP + galley.size().x,
        pad.y * 2.0 + galley.size().y.max(ICON),
    );
    // Disabled means "senses the pointer, reports nothing": the tooltip still appears, and
    // `clicked()` is false without the caller having to remember to ask.
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    // `.btn:disabled { opacity: .45 }`, and `.btn-secondary:hover` washes 7 % of the ink over
    // itself — the design's own two answers, applied to its own bare button.
    let ink = if enabled { token::TEXT } else { theme::muted(45) };
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 0.0_f32, theme::muted(if resp.is_pointer_button_down_on() { 14 } else { 7 }));
    }
    let painter = ui.painter();
    icon(painter, Pos2::new(rect.left() + pad.x + ICON * 0.5, rect.center().y), ICON, ink);
    painter.galley(
        Pos2::new(rect.left() + pad.x + ICON + GAP, rect.center().y - galley.size().y * 0.5),
        galley,
        ink,
    );

    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// The design's short vertical rule: 2 px wide, 22 px tall, in the divider ink.
///
/// Drawn rather than [`egui::Separator`], which stretches to the full height of its layout and is a
/// hairline — two differences that both read as "this bar was assembled by someone else".
pub fn rule_v(ui: &mut Ui, height: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(2.0, height), Sense::hover());
    ui.painter().rect_filled(rect, 0.0_f32, colour);
}

/// A single segment outside a group — the wire/rotate toggles, and anything else the design draws
/// as one `segBtn` on its own. Unlike [`segmented`]'s children it paints its own fill, because
/// there is no block to slide.
pub fn seg_button(ui: &mut Ui, label: &str, active: bool, seg: Seg) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), seg.font(), Color32::PLACEHOLDER);
    let pad = seg.pad();
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
    let on = ui.ctx().animate_bool_with_time(resp.id, active, MOVE_TIME);
    let fill = token::SURFACE.lerp_to_gamma(token::ACCENT, on);
    let ink = seg.idle_ink().lerp_to_gamma(token::BG, on);
    let p = ui.painter();
    p.rect_filled(rect, 0.0_f32, fill);
    p.rect_stroke(
        rect,
        0.0_f32,
        Stroke::new(token::HAIRLINE, token::DIVIDER),
        egui::StrokeKind::Inside,
    );
    if !active && resp.hovered() {
        p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
    }
    p.galley(rect.center() - galley.size() * 0.5, galley, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// [`segmented`], but filling `width` with equal shares — the design's `flex:1` option rows, where
/// two or three choices divide a dialog's full measure between them.
pub fn segmented_filling<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: egui::Id,
    current: &mut T,
    items: &[(T, &str)],
    width: f32,
) -> bool {
    let mut changed = false;
    let each = ((width - 2.0) / items.len() as f32).max(1.0);
    egui::Frame::new()
        .stroke(Stroke::new(token::HAIRLINE, token::DIVIDER))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                let slot = ui.painter().add(egui::Shape::Noop);
                let mut active_rect = None;
                for (value, label) in items {
                    let active = *value == *current;
                    let galley = ui.painter().layout_no_wrap(
                        (*label).to_owned(),
                        Seg::Small.font(),
                        Color32::PLACEHOLDER,
                    );
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(each, galley.size().y + Seg::Small.pad().y * 2.0),
                        Sense::click(),
                    );
                    if active {
                        active_rect = Some(rect);
                    }
                    let on = ui.ctx().animate_bool_with_time(resp.id, active, MOVE_TIME);
                    if !active && resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0_f32, token::WASH_HOVER);
                    }
                    ui.painter().galley(
                        rect.center() - galley.size() * 0.5,
                        galley,
                        Seg::Small.idle_ink().lerp_to_gamma(token::BG, on),
                    );
                    if resp.clicked() {
                        *current = *value;
                        changed = true;
                    }
                }
                if let Some(r) = active_rect {
                    let ctx = ui.ctx();
                    let x = ctx.animate_value_with_time(id.with("x"), r.left(), MOVE_TIME);
                    let rect = Rect::from_min_size(Pos2::new(x, r.top()), r.size());
                    ui.painter().set(slot, egui::Shape::rect_filled(rect, 0.0_f32, token::ACCENT));
                }
            });
        });
    changed
}

/// Which ink a [`tag`] carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `.tag-accent`: something the tool wants read — a count of warnings, a headline result.
    Accent,
    /// `.tag-neutral`: a fact about the thing, not a judgement of it.
    Neutral,
    /// No fill at all — used where the chip only has to name a kind.
    Bare,
    /// `.tag-outline`: an accent hairline and accent ink over nothing. The welcome screen's
    /// positioning chips are the one place the design outlines a tag instead of filling it, and
    /// they had their own private copy of this control's numbers to get it.
    Outline,
}

/// The design's `.tag` chip: 11 px, 3 × 10 padding, square corners.
pub fn tag(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    let (fill, ink, edge) = match tone {
        Tone::Accent => (token::ACCENT_100, token::ACCENT_800, Color32::TRANSPARENT),
        Tone::Neutral => (token::NEUTRAL_100, token::NEUTRAL_800, Color32::TRANSPARENT),
        Tone::Bare => (Color32::TRANSPARENT, theme::muted(40), Color32::TRANSPARENT),
        Tone::Outline => (Color32::TRANSPARENT, token::ACCENT, token::ACCENT),
    };
    // `.tag { font-size: 11px; letter-spacing: .02em; padding: 3px 10px }`.
    let galley = ui.painter().layout_job(theme::tracked(text, 0.02, theme::font::body(11.0), ink));
    let (rect, resp) =
        ui.allocate_exact_size(galley.size() + Vec2::new(20.0, 6.0), Sense::hover());
    let p = ui.painter();
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, 0.0_f32, fill);
    }
    if edge != Color32::TRANSPARENT {
        p.rect_stroke(rect, 0.0_f32, Stroke::new(token::HAIRLINE, edge), egui::StrokeKind::Inside);
    }
    p.galley(rect.center() - galley.size() * 0.5, galley, ink);
    resp
}

/// The same tag carrying a **drawn** mark, and the heavier weight the design sets on the two totals
/// in the validation screen's header (`font-weight:800`).
///
/// The design types those as `⚠ 2 warnings` and `✓ 5 passed`. Typed here they would be a tofu box
/// followed by a number: `✓` is in none of the bundled faces and `⚠` is missing from the heading
/// one — the same reason every other mark in this port is drawn.
pub fn tag_marked(
    ui: &mut Ui,
    text: &str,
    tone: Tone,
    mark: fn(&egui::Painter, Pos2, f32, Color32),
) -> Response {
    const MARK: f32 = 10.0;
    const GAP: f32 = 5.0;
    let (fill, ink) = match tone {
        Tone::Accent => (token::ACCENT_100, token::ACCENT_800),
        Tone::Neutral => (token::NEUTRAL_100, token::NEUTRAL_800),
        Tone::Bare | Tone::Outline => (Color32::TRANSPARENT, token::ACCENT),
    };
    let galley =
        ui.painter().layout_job(theme::tracked(text, 0.02, theme::font::heading(11.0), ink));
    let size = Vec2::new(
        20.0 + MARK + GAP + galley.size().x,
        6.0 + galley.size().y.max(MARK),
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, 0.0_f32, fill);
    }
    mark(p, Pos2::new(rect.left() + 10.0 + MARK * 0.5, rect.center().y), MARK, ink);
    p.galley(
        Pos2::new(rect.left() + 10.0 + MARK + GAP, rect.center().y - galley.size().y * 0.5),
        galley,
        ink,
    );
    resp
}

/// A section label — the design's `h6`: the heading face at 13 px, uppercase, tracked `.08em`, and
/// mixed back to 55 % so it sits *under* the content it names rather than competing with it.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(theme::tracked(
        &text.to_uppercase(),
        0.08,
        theme::font::h6(),
        theme::muted(55),
    )));
}

/// A panel's caption strip: the full-width surface band every workspace region wears, with its
/// title tracked at `.14em` and a 2 px rule closing it.
///
/// `right` draws whatever belongs at the far end — a filter row, a count, a toggle.
pub fn caption_strip(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(token::SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(theme::tracked(
                    &title.to_uppercase(),
                    0.14,
                    theme::font::caption(),
                    theme::muted(62),
                )));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            });
        });
    let rect = ui.min_rect();
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        ui.cursor().top() - token::RULE,
        token::RULE,
        token::DIVIDER,
    );
}

/// A full screen's header: the title at `h2`, an optional chip beside it, an optional subtitle, and
/// the 2 px rule that separates all of it from the screen's content.
///
/// Every full-page screen in the design is built this way, and every one of them had drifted to its
/// own title size (21, 25, 21, 21) with no rule at all.
pub fn screen_header(ui: &mut Ui, title: &str, chip: Option<&str>, subtitle: Option<&str>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui.label(egui::RichText::new(title).font(theme::font::h2()).color(token::TEXT));
        if let Some(chip) = chip {
            tag(ui, chip, Tone::Accent);
        }
        if let Some(sub) = subtitle {
            ui.label(egui::RichText::new(sub).font(theme::font::body(12.0)).color(theme::muted(58)));
        }
    });
    ui.add_space(token::SPACE_4);
    let rect = ui.max_rect();
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        ui.cursor().top(),
        token::RULE,
        token::DIVIDER,
    );
    ui.add_space(token::SPACE_6 - 2.0);
}

/// The centred, capped column every full-screen page sits in.
///
/// `css_max_width` is the number **written in the design** — 880 for validation, 1000 for discovery
/// — not the width of the content. The design system sets `box-sizing: border-box` (`styles.css`),
/// so a `max-width:880px` page with `padding:34px` is 812 px of content, and passing 880 straight
/// through as a content width made four of the five pages 68 px wider than the design. The
/// arithmetic belongs here, once, so that every call site can carry the design's own number.
pub fn page<R>(
    ui: &mut Ui,
    css_max_width: f32,
    pad: egui::Margin,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let content = css_max_width - f32::from(pad.left) - f32::from(pad.right);
    egui::Frame::new()
        .fill(token::BG)
        .inner_margin(pad)
        .show(ui, |ui| {
            let width = ui.available_width().min(content);
            let inset = ((ui.available_width() - width) * 0.5).max(0.0);
            let rect = Rect::from_min_size(
                Pos2::new(ui.min_rect().left() + inset, ui.cursor().top()),
                Vec2::new(width, ui.available_height()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner
        })
        .inner
}

/// The design's `.input`: a surface box with a hairline border, 6 × 10 of padding and a floor under
/// its height. Every screen was rolling its own, at four different heights.
pub fn input(ui: &mut Ui, text: &mut String, width: f32, hint: &str, mono: bool) -> Response {
    let height = theme::density_of(ui.ctx()).input_height();
    let font = if mono {
        theme::font::mono(12.0)
    } else {
        theme::font::body(13.0)
    };
    ui.add_sized(
        [width, height],
        egui::TextEdit::singleline(text)
            .font(font)
            .hint_text(hint)
            .margin(egui::Margin::symmetric(10, 6))
            .background_color(token::SURFACE),
    )
}

/// A note the design sets apart with a 2 px accent edge rather than a heading: the welcome screen's
/// strategy paragraph, the texture tab's empty state, the discovery screen's principle.
pub fn note_box(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(token::SURFACE)
        .inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(theme::font::body(11.5))
                    .color(theme::muted(72))
                    .line_height(Some(11.5 * 1.55)),
            );
        });
    theme::draw::accent_left_bar(ui.painter(), ui.min_rect());
}

/// A card: the surface tile the welcome screen's modes sit in, lifted a little further off the page
/// while the pointer is over it.
///
/// The elevation is chosen *after* the contents are laid out, which is the only way to know the
/// rectangle the pointer would have to be inside.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    let margin = egui::Margin::same(12);
    let mut prepared = egui::Frame::new()
        .fill(token::SURFACE)
        .inner_margin(margin)
        .shadow(theme::shadow::SM)
        .begin(ui);
    let inner = add(&mut prepared.content_ui);
    let rect = prepared.content_ui.min_rect() + margin;
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        prepared.frame.shadow = theme::shadow::MD;
    }
    let resp = prepared.end(ui).interact(Sense::click());
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), inner)
}

/// `.btn-primary`: the one filled button on a screen — the thing the screen is *for*.
pub fn button_primary(ui: &mut Ui, label: &str) -> Response {
    button_filled(ui, label, token::ACCENT, token::BG, token::ACCENT_600, token::ACCENT_700)
}

/// `.btn-secondary`: bordered, washed on hover, never filled.
pub fn button_secondary(ui: &mut Ui, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        theme::font::heading(14.0),
        Color32::PLACEHOLDER,
    );
    let (rect, resp) =
        ui.allocate_exact_size(galley.size() + Vec2::new(28.8, 16.0), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 0.0_f32, token::WASH_PRESS);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0_f32, token::WASH_HOVER);
    }
    p.rect_stroke(
        rect,
        0.0_f32,
        Stroke::new(token::HAIRLINE, token::DIVIDER),
        egui::StrokeKind::Inside,
    );
    p.galley(rect.center() - galley.size() * 0.5, galley, token::TEXT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn button_filled(
    ui: &mut Ui,
    label: &str,
    fill: Color32,
    ink: Color32,
    hover: Color32,
    press: Color32,
) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        theme::font::heading(14.0),
        Color32::PLACEHOLDER,
    );
    // `.btn`'s padding: `var(--space-2)` × `calc(var(--space-3) * 1.2)`.
    let (rect, resp) =
        ui.allocate_exact_size(galley.size() + Vec2::new(28.8, 16.0), Sense::click());
    let bg = if resp.is_pointer_button_down_on() {
        press
    } else if resp.hovered() {
        hover
    } else {
        fill
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0_f32, bg);
    p.galley(rect.center() - galley.size() * 0.5, galley, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A legend entry: a solid swatch and what it means.
pub fn swatch(ui: &mut Ui, colour: Color32, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0_f32, colour);
        ui.label(egui::RichText::new(label).font(theme::font::body(11.0)).color(theme::muted(60)));
    });
}

