//! The "Modernist" design tokens, carried over to egui.
//!
//! The values here are the design system's own — the ramps were generated in OKLCH on one shared
//! lightness scale, so a step of any role matches the others in visual value; don't retune them
//! one at a time. Two properties of that system make the port unusually faithful: **every radius
//! is 0** and surfaces are flat with 2 px dividers, so nothing depends on CSS gradients or
//! shadows.
//!
//! The one thing egui cannot do is `letter-spacing`, which the design uses on its small uppercase
//! labels. Those are separated by size and colour here instead.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style, Visuals,
};

/// Parse a `#rrggbb` literal at compile-ish time — keeps the palette below readable as the CSS it
/// came from.
const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The design tokens, verbatim from the system's `styles.css`.
///
/// The whole ramp is here even where no panel has reached for a step yet: the values are one
/// generated scale, and pruning it to "what is used today" would make the next screen guess.
#[allow(dead_code)]
pub mod token {
    use super::{hex, Color32};

    pub const BG: Color32 = hex(0xf3f2f2);
    pub const SURFACE: Color32 = hex(0xeae9e9);
    pub const TEXT: Color32 = hex(0x201e1d);
    pub const ACCENT: Color32 = hex(0xec3013);
    pub const ACCENT_2: Color32 = hex(0xe15b47);

    pub const NEUTRAL_100: Color32 = hex(0xf8f4f4);
    pub const NEUTRAL_200: Color32 = hex(0xeae7e7);
    pub const NEUTRAL_300: Color32 = hex(0xd7d3d3);
    pub const NEUTRAL_400: Color32 = hex(0xbab6b6);
    pub const NEUTRAL_500: Color32 = hex(0x9b9797);
    pub const NEUTRAL_600: Color32 = hex(0x7d7979);
    pub const NEUTRAL_700: Color32 = hex(0x605d5d);
    pub const NEUTRAL_800: Color32 = hex(0x444141);
    pub const NEUTRAL_900: Color32 = hex(0x2d2b2b);

    pub const ACCENT_100: Color32 = hex(0xfff2ef);
    pub const ACCENT_200: Color32 = hex(0xffe0d9);
    pub const ACCENT_300: Color32 = hex(0xffc4b8);
    pub const ACCENT_400: Color32 = hex(0xff9783);
    pub const ACCENT_500: Color32 = hex(0xff563c);
    pub const ACCENT_600: Color32 = hex(0xdd2b0f);
    pub const ACCENT_700: Color32 = hex(0xae1800);
    pub const ACCENT_800: Color32 = hex(0x7c1405);
    pub const ACCENT_900: Color32 = hex(0x4d170e);

    pub const ACCENT_2_100: Color32 = hex(0xfff2ef);
    pub const ACCENT_2_200: Color32 = hex(0xffe0da);
    pub const ACCENT_2_300: Color32 = hex(0xffc4b9);
    pub const ACCENT_2_400: Color32 = hex(0xff9784);
    pub const ACCENT_2_500: Color32 = hex(0xef6853);
    pub const ACCENT_2_600: Color32 = hex(0xc94b39);
    pub const ACCENT_2_700: Color32 = hex(0x9e3526);
    pub const ACCENT_2_800: Color32 = hex(0x71261b);
    pub const ACCENT_2_900: Color32 = hex(0x471d16);

    /// `--color-divider` is the ink at 40 % *over whatever is behind it*. A translucent stroke
    /// would land on two different colours over `BG` and `SURFACE`, and egui's premultiplied
    /// blend does not reproduce a browser's anyway — so both are baked.
    pub const DIVIDER_ON_BG: Color32 = hex(0x9f9d9d);
    pub const DIVIDER_ON_SURFACE: Color32 = hex(0x999898);
    /// The one to reach for when the background is the page itself.
    pub const DIVIDER: Color32 = DIVIDER_ON_BG;
    /// `color-mix(divider 55 %, transparent)` baked over the page: the rule *between rows of one
    /// table*, as opposed to the rule that ends a region. The design uses the two at very different
    /// strengths and mixing them up is what makes a dense list look like a grid.
    pub const DIVIDER_SOFT: Color32 = hex(0xc9c8c7);

    /// The backdrop a dialog drops over the app: `--color-neutral-900` at 45 %.
    ///
    /// Not expressible through [`super::muted`], which only ever mixes the text ink.
    pub const SCRIM: Color32 = Color32::from_rgba_premultiplied(20, 19, 19, 115);

    /// The two washes every control in the design answers the pointer with: `color-mix(text 7 %)`
    /// on hover, 14 % while held. Named because four modules were each re-deriving them.
    pub const WASH_HOVER: Color32 = Color32::from_rgba_premultiplied(2, 2, 2, 18);
    pub const WASH_PRESS: Color32 = Color32::from_rgba_premultiplied(4, 4, 4, 36);

    /// The design's spacing scale (`--space-1` … `--space-8`).
    pub const SPACE_1: f32 = 4.0;
    pub const SPACE_2: f32 = 8.0;
    pub const SPACE_3: f32 = 12.0;
    pub const SPACE_4: f32 = 16.0;
    pub const SPACE_6: f32 = 24.0;
    pub const SPACE_8: f32 = 32.0;

    /// The structural rule that ends a region — a bar, a header, a table's head. 2 px.
    pub const RULE: f32 = 2.0;
    /// The outline of a *control* — a button, an input, a segment box. 1 px.
    ///
    /// The 1-versus-2 distinction is the design's main structural rhythm, so both widths are named
    /// rather than remembered at each call site.
    pub const HAIRLINE: f32 = 1.0;
}

/// The elevation scale (`--shadow-sm/md/lg`), as egui shadows.
///
/// All three have zero horizontal offset and are tinted with the palette's darkest neutral rather
/// than black — egui's own defaults are offset to the right and pure black, which reads as a
/// different interface as soon as any menu opens.
pub mod shadow {
    use egui::epaint::Shadow;
    use egui::Color32;

    /// `--color-neutral-900` (#2d2b2b) at `percent`, premultiplied — the ink these shadows are cast
    /// in. Written out rather than computed because a `const` cannot call `gamma_multiply`.
    const fn neutral_900(r: u8, g: u8, b: u8, a: u8) -> Color32 {
        Color32::from_rgba_premultiplied(r, g, b, a)
    }

    /// `0 1px 2px … 14%`
    pub const SM: Shadow =
        Shadow { offset: [0, 1], blur: 2, spread: 0, color: neutral_900(6, 6, 6, 36) };
    /// `0 3px 10px … 16%`
    pub const MD: Shadow =
        Shadow { offset: [0, 3], blur: 10, spread: 0, color: neutral_900(7, 7, 7, 41) };
    /// `0 12px 32px … 22%`
    pub const LG: Shadow =
        Shadow { offset: [0, 12], blur: 32, spread: 0, color: neutral_900(10, 9, 9, 56) };
}

/// `--color-text` mixed with transparency, the way the design's `color-mix(… N%, transparent)`
/// reads: a muted ink rather than a separate grey.
#[must_use]
pub fn muted(percent: u8) -> Color32 {
    token::TEXT.gamma_multiply(f32::from(percent) / 100.0)
}

/// How tightly the interface is packed. The design ships this as a user setting, and in egui it is
/// a cleaner switch than in CSS: one spacing struct drives every panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Density {
    /// "Small": the design's own setting — a data tool is read, not browsed. Right once the tool is
    /// familiar, cramped for a first look, which is why it is not the default.
    Compact,
    /// "Medium": the default.
    #[default]
    Balanced,
    /// "Large".
    Roomy,
}

impl Density {
    /// Row height for the dense list panels (tree, hex, log) — the design's `D.rh`.
    #[must_use]
    pub fn row_height(self) -> f32 {
        match self {
            Self::Compact => 22.0,
            Self::Balanced => 27.0,
            Self::Roomy => 33.0,
        }
    }

    /// Base body size (`D.tfs`); every other size in the app is derived from the design's own scale.
    #[must_use]
    pub fn body_size(self) -> f32 {
        match self {
            Self::Compact => 12.5,
            Self::Balanced => 13.5,
            Self::Roomy => 14.5,
        }
    }

    /// Monospace size (`D.hfs`) — the hex dump, the tree's offsets, every number in a list.
    ///
    /// Its own step rather than "the body minus a half": the design tunes the two independently,
    /// and four panels were each subtracting a different amount from [`Self::body_size`].
    #[must_use]
    pub fn mono_size(self) -> f32 {
        match self {
            Self::Compact => 11.5,
            Self::Balanced => 12.5,
            Self::Roomy => 13.5,
        }
    }

    /// The design's `D.fss`: field labels, captions, anything secondary.
    #[must_use]
    pub fn small_size(self) -> f32 {
        match self {
            Self::Compact => 11.0,
            Self::Balanced => 12.0,
            Self::Roomy => 12.5,
        }
    }

    /// `.input`'s `min-height: 36px`, at the three settings.
    #[must_use]
    pub fn input_height(self) -> f32 {
        match self {
            Self::Compact => 32.0,
            Self::Balanced => 36.0,
            Self::Roomy => 40.0,
        }
    }

    /// The floor under every interactive widget — the design's smallest button box.
    ///
    /// Distinct from [`Self::row_height`], which it used to share: a list row and a button are not
    /// the same object, and tying them together made every control in the app 17 px tall.
    #[must_use]
    pub fn control_height(self) -> f32 {
        match self {
            Self::Compact => 22.0,
            Self::Balanced => 24.0,
            Self::Roomy => 28.0,
        }
    }

    /// Width of the chunk-tree column (`D.treeW`).
    #[must_use]
    pub fn tree_width(self) -> f32 {
        match self {
            Self::Compact => 242.0,
            Self::Balanced => 266.0,
            Self::Roomy => 290.0,
        }
    }

    /// Width of the inspector column (`D.inspW`).
    #[must_use]
    pub fn inspector_width(self) -> f32 {
        match self {
            Self::Compact => 288.0,
            Self::Balanced => 314.0,
            Self::Roomy => 342.0,
        }
    }

    /// Height of the log strip (`D.logH`).
    #[must_use]
    pub fn log_height(self) -> f32 {
        match self {
            Self::Compact => 150.0,
            Self::Balanced => 176.0,
            Self::Roomy => 202.0,
        }
    }

    /// Padding inside a panel, from the design's spacing scale.
    #[must_use]
    pub fn pad(self) -> f32 {
        match self {
            Self::Compact => token::SPACE_1,
            Self::Balanced => token::SPACE_2,
            Self::Roomy => token::SPACE_3,
        }
    }

    /// Every setting, for a settings panel that offers them all at once rather than cycling.
    #[must_use]
    pub fn all() -> [Self; 3] {
        [Self::Compact, Self::Balanced, Self::Roomy]
    }

}

/// Named text roles, so a panel asks for "the tree's monospace" rather than a number.
pub mod font {
    use egui::{FontFamily, FontId};

    /// The heading family (Archivo ExtraBold) — the design's signature weight.
    pub fn heading_family() -> FontFamily {
        FontFamily::Name("heading".into())
    }

    /// A heading of `size` px.
    pub fn heading(size: f32) -> FontId {
        FontId::new(size, heading_family())
    }

    /// Monospace, for everything that is bytes, offsets or identifiers.
    pub fn mono(size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// Body text of `size` px.
    pub fn body(size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    /// The design's heading ladder (`h1` … `h6`), so a screen asks for a *step* rather than for a
    /// number it picked. Every screen title in the design is an `h2`; every section label an `h6`.
    ///
    /// Kept whole. Three of the six steps are not currently asked for by any screen, and a ladder
    /// with rungs missing is worse than one with rungs unused: the next screen that wants a size
    /// between `h2` and `h4` should find `h3` here rather than type `25.0` and start the drift this
    /// module exists to stop.
    #[allow(dead_code)]
    pub fn h1() -> FontId {
        heading(42.0)
    }
    pub fn h2() -> FontId {
        heading(32.0)
    }
    #[allow(dead_code)]
    pub fn h3() -> FontId {
        heading(25.0)
    }
    pub fn h4() -> FontId {
        heading(20.0)
    }
    #[allow(dead_code)]
    pub fn h5() -> FontId {
        heading(16.0)
    }
    pub fn h6() -> FontId {
        heading(13.0)
    }

    /// The panel caption: 10 px of the heading face, and the design's `.14em` of tracking behind
    /// it — see [`super::tracked`], which is what actually widens it.
    pub fn caption() -> FontId {
        heading(10.0)
    }
}

/// Text with the design's letter-spacing, as a layout job.
///
/// The small uppercase labels — `TREE`, `INSPECTOR`, `LOG`, every `h6` — are tracked between
/// `.06em` and `.14em`, and that tracking is the most recognisable typographic move in the system:
/// set flush, `INSPECTOR` comes out visibly narrower and tighter than the design's. egui has no
/// per-glyph tracking in its styles, but `epaint` carries `extra_letter_spacing` per text *section*.
///
/// **One section for the whole string, not one per character.** `epaint` adds the spacing before
/// each glyph that has a predecessor *within its own section* (`text_layout.rs`: `last_glyph_id`
/// is declared inside `layout_section`, so it starts as `None` for every section). A section per
/// character therefore has no predecessor anywhere and the spacing is added exactly zero times —
/// which is how this drew every tracked label in the interface flush while claiming to track it.
/// One section spaces the `n - 1` gaps and leaves no tail, which is CSS `letter-spacing` without
/// the trailing gap CSS also adds.
#[must_use]
pub fn tracked(text: &str, em: f32, font: FontId, colour: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color: colour,
            extra_letter_spacing: em * font.size,
            ..Default::default()
        },
    );
    job
}

/// Status marks, drawn rather than typed.
///
/// `⚠` exists in the bundled fallbacks but `✓`/`✕` do not, and a missing glyph renders as a tofu
/// box — on a validation screen, the one mark that must be unmistakable. Two line segments are
/// cheaper than shipping another font.
pub mod mark {
    use super::token;
    use egui::{Color32, Painter, Pos2, Stroke, Vec2};

    /// A tick centred on `at`, `size` across.
    pub fn ok(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let s = size * 0.5;
        let stroke = Stroke::new((size * 0.16).max(1.2), colour);
        let a = at + Vec2::new(-s, 0.0);
        let b = at + Vec2::new(-s * 0.25, s * 0.7);
        let c = at + Vec2::new(s, -s * 0.75);
        p.line_segment([a, b], stroke);
        p.line_segment([b, c], stroke);
    }

    /// A cross centred on `at`.
    pub fn error(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let s = size * 0.42;
        let stroke = Stroke::new((size * 0.16).max(1.2), colour);
        p.line_segment([at + Vec2::new(-s, -s), at + Vec2::new(s, s)], stroke);
        p.line_segment([at + Vec2::new(-s, s), at + Vec2::new(s, -s)], stroke);
    }

    /// A hollow ring — "nothing read here", which is not a pass.
    pub fn unchecked(p: &Painter, at: Pos2, size: f32) {
        p.circle_stroke(at, size * 0.34, Stroke::new(1.0_f32, token::NEUTRAL_400));
    }

    /// A warning triangle, centred on `at`.
    ///
    /// `⚠` does reach the screen through egui's bundled emoji fallback, but only in the families
    /// that *have* fallbacks — the heading family is one bundled face and nothing else, so the
    /// design's `font-weight:800` warning renders there as a tofu box. Drawing it settles that, and
    /// puts it in the same ink and weight as the tick it stands next to.
    pub fn warn(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let s = size * 0.5;
        let stroke = Stroke::new((size * 0.14).max(1.2), colour);
        p.add(egui::Shape::line(
            vec![
                at + Vec2::new(0.0, -s * 0.95),
                at + Vec2::new(s * 0.95, s * 0.75),
                at + Vec2::new(-s * 0.95, s * 0.75),
                at + Vec2::new(0.0, -s * 0.95),
            ],
            stroke,
        ));
        p.line_segment([at + Vec2::new(0.0, -s * 0.3), at + Vec2::new(0.0, s * 0.2)], stroke);
        p.circle_filled(at + Vec2::new(0.0, s * 0.48), stroke.width * 0.6, colour);
    }
}

/// The top bar's two action icons, drawn for the same reason the status marks are.
///
/// The design sets them as Lucide outlines — `folder-open` and `download` — and Archivo carries
/// neither, so a typed `🗀`/`⤓` would fall through to whatever fallback the machine happens to have,
/// or to a tofu box. A dozen line segments cost less than bundling an icon font for two glyphs.
///
/// Both are transcribed from the design's own `<svg>` rather than drawn by eye: the vertices below
/// are the SVG path's, in its 24 × 24 viewBox, mapped by [`pt`]. The paths' 2-unit corner radii are
/// dropped — at the 15 px the design asks for they are five thirds of a *pixel*, and a straight
/// corner there is indistinguishable from an arc while being far easier to read as code.
pub mod icon {
    use egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2};

    /// A point of the design's 24 × 24 viewBox, in screen space: `at` is where the middle of the
    /// box lands, `size` how wide the whole box is drawn.
    fn pt(at: Pos2, size: f32, x: f32, y: f32) -> Pos2 {
        at + Vec2::new(x - 12.0, y - 12.0) * (size / 24.0)
    }

    /// Lucide `folder-open`: the folder's back with its tab, and the front swung out to the right.
    ///
    /// One continuous path in the design, and one here — the lean of the front face is the whole
    /// reason the shape reads as *open*, so it is the one thing worth checking if it ever looks off.
    pub fn open(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x: f32, y: f32| pt(at, size, x, y);
        p.add(Shape::line(
            vec![
                v(6.0, 14.0),
                v(7.5, 11.1),
                v(9.24, 10.0),
                v(20.0, 10.0),
                v(21.94, 12.5),
                v(20.39, 18.5),
                v(18.45, 20.0),
                v(4.0, 20.0),
                v(2.0, 18.0),
                v(2.0, 5.0),
                v(4.0, 3.0),
                v(7.9, 3.0),
                v(9.59, 3.9),
                v(10.4, 5.1),
                v(12.07, 6.0),
                v(18.0, 6.0),
                v(20.0, 8.0),
                v(20.0, 10.0),
            ],
            stroke(size, colour),
        ));
    }

    /// Lucide `download`: an arrow coming down into an open tray — the direction the *file* moves,
    /// not the direction the tool is looking.
    pub fn export(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x: f32, y: f32| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        // The tray, open at the top: `M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4`.
        p.add(Shape::line(
            vec![v(21.0, 15.0), v(21.0, 19.0), v(19.0, 21.0), v(5.0, 21.0), v(3.0, 19.0), v(3.0, 15.0)],
            stroke,
        ));
        p.add(Shape::line(vec![v(7.0, 10.0), v(12.0, 15.0), v(17.0, 10.0)], stroke));
        p.line_segment([v(12.0, 15.0), v(12.0, 3.0)], stroke);
    }

    /// Lucide `upload`: the same tray, and the arrow going the other way.
    ///
    /// Deliberately [`export`] mirrored rather than a different shape. The two actions sit next to
    /// each other under one preview — write this image out, put that image in — and the only thing
    /// that differs between them is the direction the file moves, which is exactly the only thing
    /// that differs between the two icons. The design draws neither; it has no import anywhere.
    pub fn import(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        let v = |x: f32, y: f32| pt(at, size, x, y);
        let stroke = stroke(size, colour);
        p.add(Shape::line(
            vec![v(21.0, 15.0), v(21.0, 19.0), v(19.0, 21.0), v(5.0, 21.0), v(3.0, 19.0), v(3.0, 15.0)],
            stroke,
        ));
        p.add(Shape::line(vec![v(7.0, 8.0), v(12.0, 3.0), v(17.0, 8.0)], stroke));
        p.line_segment([v(12.0, 3.0), v(12.0, 15.0)], stroke);
    }

    /// The design's `stroke-width: 2` in the same 24-unit box, but never thin enough to vanish.
    fn stroke(size: f32, colour: Color32) -> Stroke {
        Stroke::new((size * 2.0 / 24.0).max(1.1), colour)
    }
}

/// The marks the design draws with shapes rather than with type or with a widget.
///
/// Each of these appears on three or more screens; keeping them here is what stops the dashed
/// border on the drop zone and the dot beside a recent file from drifting apart by a pixel.
pub mod draw {
    use super::token;
    use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

    /// The design's `2px dashed` outline, which `egui::Stroke` cannot express.
    pub fn dashed_rect(p: &Painter, rect: Rect, width: f32, colour: Color32) {
        let stroke = Stroke::new(width, colour);
        let (dash, gap) = (6.0_f32, 4.0_f32);
        for (a, b) in [
            (rect.left_top(), rect.right_top()),
            (rect.right_top(), rect.right_bottom()),
            (rect.right_bottom(), rect.left_bottom()),
            (rect.left_bottom(), rect.left_top()),
        ] {
            p.add(Shape::dashed_line(&[a, b], stroke, dash, gap));
        }
    }

    /// The 8 px status dot: accent for "this is the one", neutral for the rest.
    pub fn dot(p: &Painter, at: Pos2, size: f32, colour: Color32) {
        p.circle_filled(at, size * 0.5, colour);
    }

    /// The 2 px accent bar down the left edge of a note, a callout or a floating panel — the
    /// design's way of saying "this block is worth reading" without a heading.
    pub fn accent_left_bar(p: &Painter, rect: Rect) {
        p.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(token::RULE, rect.height())),
            0.0_f32,
            token::ACCENT,
        );
    }

    /// A full-width structural rule — the 2 px line that closes a header, a caption or a table's
    /// head. `width` is the design's `RULE` unless a call site says otherwise.
    pub fn rule_h(p: &Painter, x: std::ops::RangeInclusive<f32>, y: f32, width: f32, colour: Color32) {
        p.rect_filled(
            Rect::from_min_max(Pos2::new(*x.start(), y), Pos2::new(*x.end(), y + width)),
            0.0_f32,
            colour,
        );
    }

    /// The transparency checkerboard an image with alpha sits on: 22 px squares in the palette's two
    /// lightest neutrals, so a fully transparent texture still reads as *something*.
    pub fn checker(p: &Painter, rect: Rect, square: f32) {
        p.rect_filled(rect, 0.0_f32, token::NEUTRAL_200);
        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { square };
            while x < rect.right() {
                let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(square))
                    .intersect(rect);
                p.rect_filled(cell, 0.0_f32, token::NEUTRAL_300);
                x += square * 2.0;
            }
            y += square;
            row += 1;
        }
    }
}

/// Install Archivo (bundled, SIL OFL) as the proportional and heading families.
///
/// The fonts are compiled into the binary so the tool looks the same on a machine that has never
/// heard of Archivo; if egui ever fails to parse them it keeps its own defaults rather than
/// refusing to start.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "archivo".to_owned(),
        std::sync::Arc::new(FontData::from_static(include_bytes!("../assets/Archivo-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "archivo-bold".to_owned(),
        std::sync::Arc::new(FontData::from_static(include_bytes!("../assets/Archivo-ExtraBold.ttf"))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "archivo".to_owned());
    // The heading family keeps egui's own fallbacks behind Archivo, exactly as Proportional does
    // above. Without them a heading that reaches for a glyph Archivo ExtraBold lacks — `⚠`, an
    // arrow, a box-drawing rule — renders as a tofu box instead of falling through.
    let mut heading = vec!["archivo-bold".to_owned()];
    if let Some(rest) = fonts.families.get(&FontFamily::Proportional) {
        heading.extend(rest.iter().filter(|f| *f != "archivo").cloned());
    }
    fonts.families.insert(FontFamily::Name("heading".into()), heading);
    ctx.set_fonts(fonts);
}

/// Apply the design system to a context: palette, zero radii, hairline strokes, and the text
/// sizes for the given density.
pub fn apply(ctx: &egui::Context, density: Density) {
    let mut visuals = Visuals::light();

    visuals.panel_fill = token::BG;
    visuals.window_fill = token::SURFACE;
    visuals.extreme_bg_color = token::SURFACE;
    visuals.faint_bg_color = token::NEUTRAL_200;
    visuals.override_text_color = Some(token::TEXT);
    visuals.hyperlink_color = token::ACCENT;
    visuals.selection.bg_fill = token::ACCENT.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0_f32, token::ACCENT);
    // `.dialog` is a surface and a shadow, with no border at all — the 2 px outline egui would
    // otherwise draw round the settings menu is not in this design.
    visuals.window_stroke = Stroke::NONE;
    visuals.popup_shadow = shadow::MD;
    visuals.window_shadow = shadow::LG;
    // egui's own light theme keeps an orange warning, a pure red error and a grey code wash — three
    // colours this palette does not contain, on paths any egui-internal message can surface.
    visuals.warn_fg_color = token::ACCENT;
    visuals.error_fg_color = token::ACCENT_700;
    visuals.code_bg_color = token::SURFACE;
    // `.input { caret-color: var(--color-accent) }`. The default is a blue left over from egui.
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, token::ACCENT);

    // Every radius in the system is 0 — the flat, squared-off look is the design's signature.
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::ZERO;
    }
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;

    // This is the stroke egui rules every panel edge and every `separator()` with, and in the design
    // a region always ends in 2 px. Controls keep the hairline, stated at their own call sites.
    visuals.widgets.noninteractive.bg_fill = token::BG;
    visuals.widgets.noninteractive.weak_bg_fill = token::BG;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(token::RULE, token::DIVIDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, token::TEXT);

    visuals.widgets.inactive.bg_fill = token::SURFACE;
    visuals.widgets.inactive.weak_bg_fill = token::SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(token::HAIRLINE, token::DIVIDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, token::TEXT);

    // Hover is a wash and nothing else: the border stays the divider, and the accent is reserved
    // for what is *focused* or *selected*. An accent outline round everything the pointer passes
    // over is the loudest thing this interface could do.
    visuals.widgets.hovered.bg_fill = token::WASH_HOVER;
    visuals.widgets.hovered.weak_bg_fill = token::WASH_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(token::HAIRLINE, muted(45));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, token::TEXT);

    // Pressed *darkens* — `.btn-primary:active` is a fill change, not an outline change.
    visuals.widgets.active.bg_fill = token::ACCENT_700;
    visuals.widgets.active.weak_bg_fill = token::ACCENT_700;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, token::BG);

    let mut style = Style { visuals, ..Default::default() };
    style.text_styles = [
        (egui::TextStyle::Heading, font::h4()),
        (egui::TextStyle::Body, font::body(density.body_size())),
        (egui::TextStyle::Monospace, font::mono(density.mono_size())),
        // `.btn` is the heading face at 14 px — every stock button in the app was falling through
        // to the body face, which is the one weight this design never sets a button in.
        (egui::TextStyle::Button, font::heading(14.0)),
        (egui::TextStyle::Small, font::body(density.small_size())),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(token::SPACE_2, density.pad());
    // `.btn { padding: var(--space-2) calc(var(--space-3) * 1.2) }` — 8 vertical, 14.4 horizontal,
    // and density-independent, which is how the design writes it.
    style.spacing.button_padding = egui::vec2(token::SPACE_3 * 1.2, token::SPACE_2);
    style.spacing.window_margin = egui::Margin::same(0);
    style.spacing.interact_size.y = density.control_height();
    style.spacing.scroll.bar_width = 9.0; // the design's own scrollbar width
    // A solid bar in the palette's greys: egui's floating default fades out over the content, which
    // in a tool built of dense lists reads as a rendering fault rather than as restraint.
    style.spacing.scroll.floating = false;
    style.spacing.scroll.foreground_color = true;
    style.spacing.scroll.handle_min_length = 24.0;
    ctx.set_global_style(style);
    ctx.data_mut(|d| d.insert_temp(DENSITY_ID.with("v"), density));
}

/// Where [`apply`] leaves the current [`Density`] for widgets to find.
const DENSITY_ID: egui::Id = egui::Id::NULL;

/// What the interface is currently set to, for a *widget* that needs one of the design's measures
/// and has no `PryHub` to ask.
///
/// `Style::spacing` can only carry the measures egui itself understands, and the design's input box
/// is not one of them: `interact_size.y` is the **control** height (22/24/28), so `widget::input`
/// clamping it with `.max(28.0)` came out as a constant 28 px at every setting — the one control
/// the Small/Medium/Large switch left standing still.
#[must_use]
pub fn density_of(ctx: &egui::Context) -> Density {
    ctx.data(|d| d.get_temp::<Density>(DENSITY_ID.with("v"))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings panel offers all three at once, smallest first, so the list reads as a scale.
    #[test]
    fn every_setting_is_offered_smallest_first() {
        assert_eq!(Density::all(), [Density::Compact, Density::Balanced, Density::Roomy]);
    }

    /// Medium is the default: the design's own compact setting is right once the tool is familiar
    /// and cramped for a first look.
    #[test]
    fn the_default_size_is_medium() {
        assert_eq!(Density::default(), Density::Balanced);
    }

    #[test]
    fn denser_settings_are_actually_denser() {
        assert!(Density::Compact.row_height() < Density::Balanced.row_height());
        assert!(Density::Balanced.row_height() < Density::Roomy.row_height());
        assert!(Density::Compact.body_size() < Density::Roomy.body_size());
    }

    /// The regression this exists to stop: the tracking was written as one section per character,
    /// and `epaint` only ever adds `extra_letter_spacing` *between* two glyphs of the same section —
    /// so every tracked label in the interface was set flush while the code said otherwise. Assert
    /// the shape of the job rather than a measured width: the shape is the whole mechanism, and it
    /// needs no font context to check.
    #[test]
    fn tracking_is_one_section_over_the_whole_label() {
        let job = tracked("INSPECTOR", 0.14, font::caption(), token::TEXT);
        assert_eq!(job.sections.len(), 1, "one section per character adds the spacing nowhere");
        assert_eq!(job.text, "INSPECTOR");
        let spacing = job.sections[0].format.extra_letter_spacing;
        assert!((spacing - 0.14 * font::caption().size).abs() < f32::EPSILON, "{spacing}");
    }

    #[test]
    fn muted_ink_stays_on_the_text_hue() {
        // The design mixes the ink with transparency rather than switching to a grey, so a muted
        // label must keep the text colour's own hue.
        let m = muted(55);
        assert_eq!((m.r() > m.b(), m.g() > m.b()), (true, true));
        assert!(m.a() < token::TEXT.a() || m != token::TEXT);
    }
}
