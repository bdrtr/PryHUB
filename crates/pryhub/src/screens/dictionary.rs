//! The dictionary screen: every hash the open file references, and what it is called.
//!
//! The file names almost nothing — a texture carries a truncated `DebugName`, a shader and a solid
//! carry nothing at all — so this screen is where names are kept. What makes it more than a notes
//! field is that a name can be **checked**: `gizmo_nfs::hash` hashes it back, and a name that
//! reproduces the file's number is proof rather than a note. It is also the only way a truncated
//! name's tail comes back: type candidates until one hashes.
//!
//! The design lays it out as one `.table` in a 760 px page: `hash | name | source`, with an add row
//! under it for a hash you already know but the open file never mentions.

use crate::app::PryHub;
use crate::theme::{self, token};
use crate::widget;
use egui::{Align, Label, Layout, Margin, Rect, RichText, ScrollArea, Sense, Shape, TextEdit, Vec2};
use std::collections::BTreeMap;

/// The design's page measure: a centred column, whatever the window happens to be.
const PAGE_W: f32 = 760.0;
/// `padding:30px 34px 40px`, beside the `max-width` above.
const PAGE_PAD: egui::Margin = egui::Margin { left: 34, right: 34, top: 30, bottom: 40 };
/// `.table`'s two pinned columns (`th` widths); the name takes whatever is left between them.
const HASH_W: f32 = 160.0;
const SOURCE_W: f32 = 90.0;
/// The verification mark, at the trailing edge of the name cell.
const MARK: f32 = 16.0;
/// The add row's `margin-top`, and the height a `.btn` needs — a 14 px heading galley with the
/// design's 8 px above and below it. The table's scroller has to leave both behind, and it is sized
/// before either is drawn.
const ADD_GAP: f32 = 14.0;
const ADD_H: f32 = 34.0;

/// The screen's own state.
#[derive(Default)]
pub struct State {
    /// Substring filter over hashes and names.
    pub filter: String,
    /// In-progress edits, by hash: a row is committed when it loses focus or Enter is pressed, not
    /// on every keystroke — otherwise the file would be rewritten per character typed.
    edits: BTreeMap<u32, String>,
    /// The last save, to show where the file went.
    note: Option<String>,
    /// The add row's two fields.
    new_hash: String,
    new_name: String,
    /// Hashes typed in by hand. They belong to the dictionary rather than to any file, so without
    /// this they would vanish the moment they were added — and they are the only rows a screen with
    /// no file open has to show.
    added: Vec<u32>,
}

/// Where a hash was referenced from.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct Sources {
    texture: bool,
    material: bool,
    shader: bool,
    part: bool,
}

/// One row: a hash, where it came from, and the name the file gave it (if any).
struct Row {
    hash: u32,
    sources: Sources,
    /// The name in the file — a TPK `DebugName`, or a part's name. Possibly truncated.
    file_name: Option<String>,
}

/// The table's grid, worked out once a frame. The design pins two columns and lets the name have
/// the rest, so the head and every row have to be told the same numbers.
#[derive(Clone, Copy)]
struct Grid {
    name_w: f32,
    row_h: f32,
    input_h: f32,
    /// `.table td { padding: var(--space-2) }`, at the current density.
    pad: f32,
}

/// Draw the screen.
pub fn show(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    egui::CentralPanel::default().frame(egui::Frame::new().fill(token::BG)).show_inside(
        ui,
        |ui| {
            // The design's screen is `overflow:auto`. Without it the add row at the foot of the
            // page had nowhere to go on a short window and was simply cut off by the panel.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widget::page(ui, PAGE_W, PAGE_PAD, |ui| {
                // The design's subtitle is one line on the h2's baseline row; the long explanation
                // this screen used to carry under its title cannot sit there, so it becomes what
                // the header answers a hover with.
                ui.scope(|ui| widget::screen_header(ui, t.nav_dict, None, Some(t.h_sub)))
                    .response
                    .on_hover_text(t.dc_hint);

                let rows = collect(app);
                meta(app, ui, &rows);
                ui.add_space(20.0); // `.table { margin-top: 20px }`
                table(app, ui, &rows);
                ui.add_space(ADD_GAP);
                add_row(app, ui);
            });
            });
        },
    );
}

/// The counters above the table, and the filter that narrows it.
///
/// Not in the design — the mock has five rows where a real file has hundreds — so it is set as a
/// meta line rather than as another band of interface.
fn meta(app: &mut PryHub, ui: &mut egui::Ui, rows: &[Row]) {
    let t = app.lang.strings();
    let named = rows.iter().filter(|r| app.names.get(r.hash).is_some()).count();
    let verified = rows
        .iter()
        .filter(|r| {
            app.names.get(r.hash).is_some_and(|n| gizmo_nfs::hash::string_hash(n) == r.hash)
        })
        .count();
    let font = theme::font::mono(11.0);
    ui.horizontal(|ui| {
        let count = |ui: &mut egui::Ui, text: String, ink: egui::Color32| {
            ui.label(RichText::new(text).font(font.clone()).color(ink));
        };
        count(ui, format!("{} {}", rows.len(), t.dc_hashes.of(rows.len())), theme::muted(55));
        count(ui, "·".to_owned(), theme::muted(30));
        count(ui, format!("{named} {}", t.dc_named), theme::muted(55));
        count(ui, "·".to_owned(), theme::muted(30));
        // The tick count is what the screen exists for, so it is the one number in the accent —
        // and only once there is something to put there.
        let proof = if verified == 0 { theme::muted(55) } else { token::ACCENT };
        count(ui, format!("{verified} {}", t.dc_verified), proof);
        count(ui, "·".to_owned(), theme::muted(30));
        count(ui, format!("{} {}", app.names.len(), t.dc_in_dictionary), theme::muted(55));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widget::input(ui, &mut app.dict.filter, 180.0, t.dc_filter, true);
        });
    });
    if let Some(note) = &app.dict.note {
        ui.label(RichText::new(note).font(theme::font::mono(10.5)).color(theme::muted(50)));
    }
}

/// Every hash the open file points at: textures, material runs, shaders, and the solids themselves.
fn collect(app: &mut PryHub) -> Vec<Row> {
    let mut seen: BTreeMap<u32, Row> = BTreeMap::new();
    let mut add = |hash: u32, name: Option<String>, tag: fn(&mut Sources)| {
        if hash == 0 {
            return; // an empty slot, not a reference
        }
        let row = seen.entry(hash).or_insert_with(|| Row {
            hash,
            sources: Sources::default(),
            file_name: name.clone(),
        });
        tag(&mut row.sources);
        // A name from the file is worth keeping whichever reference brought it in.
        if row.file_name.is_none() {
            row.file_name = name;
        }
    };

    // Textures first, because they are the only place the file leaves a name. They may still be
    // being read — the table simply grows when they arrive.
    //
    // Names, not pixels. This screen shows a hash and what the file calls it, and never draws an
    // image, so it asks for the one job that reads the `DebugName` out of each blob and stops. It
    // used to call `want_textures()`, which decodes every image in the pack — for a `VINYLS.BIN`
    // that is the whole 256 MB budget's worth of RGBA8, allocated and held to read 1,786 strings.
    app.want_texture_names();
    if let Some(names) = app.texture_names.clone() {
        for (hash, name) in names.iter() {
            let name = (!name.is_empty()).then(|| name.clone());
            add(hash.0, name, |s| s.texture = true);
        }
    }
    // The pack the texture tab may already have decoded is still worth listing: it carries every
    // declared entry, including the ones whose header would not give a name.
    if let Some(tpk) = app.textures.ready() {
        for entry in &tpk.entries {
            add(entry.hash.0, None, |s| s.texture = true);
        }
    }
    if let Some(doc) = &app.doc {
        for part in &doc.parts {
            let name = (!part.name.is_empty()).then(|| part.name.clone());
            add(part.hash.0, name, |s| s.part = true);
            for run in &part.materials {
                add(run.hash.0, None, |s| s.material = true);
                add(run.shader.0, None, |s| s.shader = true);
            }
        }
    }
    // Hashes typed in by hand come from the dictionary, not from the file, so they are listed
    // whether or not anything is open.
    for hash in &app.dict.added {
        add(*hash, None, |_s: &mut Sources| {});
    }
    seen.into_values().collect()
}

/// One `.table td`: a fixed-width box with the design's cell padding, so every row's input starts at
/// the same x however long the hash or the name beside it happens to be.
fn cell<R>(ui: &mut egui::Ui, w: f32, h: f32, pad: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(Vec2::new(w, h), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(Vec2::new(w, h));
        ui.add_space(pad);
        add(ui)
    })
    .inner
}

/// The `<thead>` row: the three column names, and the 2 px rule that closes them.
fn head(ui: &mut egui::Ui, t: &crate::i18n::Strings, g: Grid) {
    ui.add_space(g.pad);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // `hash` is the design's own literal: a column named after the number it holds reads the
        // same in every language.
        for (label, w) in [("hash", HASH_W), (t.h_name, g.name_w), (t.h_source, SOURCE_W)] {
            cell(ui, w, 0.0, g.pad, |ui| {
                ui.add(Label::new(theme::tracked(
                    &label.to_uppercase(),
                    0.08,
                    theme::font::mono(11.0),
                    theme::muted(60),
                )));
            });
        }
    });
    ui.add_space(g.pad);
    let rect = ui.max_rect();
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        ui.cursor().top(),
        token::RULE,
        token::DIVIDER,
    );
    ui.add_space(token::RULE);
}

/// The table: one editable name per hash, and the mark that says whether it is proof or a note.
fn table(app: &mut PryHub, ui: &mut egui::Ui, rows: &[Row]) {
    let t = app.lang.strings();
    // The design writes a hash as `0x8F3A21C0`, so the filter accepts it written that way too.
    let filter = app.dict.filter.trim().to_uppercase();
    let filter = filter.strip_prefix("0X").unwrap_or(&filter);
    let visible: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            filter.is_empty()
                || format!("{:08X}", r.hash).contains(filter)
                || r.file_name.as_ref().is_some_and(|n| n.to_uppercase().contains(filter))
                || app.names.get(r.hash).is_some_and(|n| n.to_uppercase().contains(filter))
        })
        .collect();

    let pad = app.density.pad();
    // The design shrinks `.input` from its 36 px floor to 30 inside a table row; keep those 6 px at
    // every density rather than pinning the table to one setting's numbers.
    let input_h = (app.density.input_height() - 6.0).max(20.0);
    let row_h = input_h + pad * 2.0;
    // What still has to fit under the table: the add row, the gap above it, and the page's own
    // bottom padding. Leaving the padding out is what pushed the add row past the panel's floor,
    // where it was clipped rather than scrolled to.
    let table_h = (ui.available_height() - ADD_GAP - ADD_H - f32::from(PAGE_PAD.bottom))
        .max(row_h * 3.0);
    // The scrollbar takes layout width the moment it appears, and the head is drawn outside the
    // scroller — so the elastic column has to know about it or the two grids come apart.
    let bar = if visible.len() as f32 * row_h > table_h {
        ui.spacing().scroll.bar_width
    } else {
        0.0
    };
    let g = Grid {
        name_w: (ui.available_width() - bar - HASH_W - SOURCE_W).max(140.0),
        row_h,
        input_h,
        pad,
    };

    head(ui, t, g);

    if visible.is_empty() {
        // The dictionary is not a view of the open file, so an empty one is only worth explaining
        // when there is nothing open to explain it with.
        if app.doc.is_none() {
            ui.add_space(pad);
            ui.label(RichText::new(t.no_file).font(theme::font::body(12.0)).color(theme::muted(50)));
        }
        return;
    }

    let commit = ui
        .scope(|ui| {
            // `.table` rows touch: the 1 px rule between them is the only gap, and `show_rows`
            // reserves exactly what the closure paints only once the spacing is out of the way.
            ui.spacing_mut().item_spacing.y = 0.0;
            ScrollArea::vertical()
                .max_height(table_h)
                .auto_shrink([false, false])
                .show_rows(ui, row_h, visible.len(), |ui, range| {
                    let mut commit = None;
                    for r in visible[range].iter().copied() {
                        if let Some(settled) = row(app, ui, r, g) {
                            commit = Some(settled);
                        }
                    }
                    commit
                })
                .inner
        })
        .inner;

    if let Some((hash, name)) = commit {
        app.names.set(hash, &name);
        save(app);
    }
}

/// One row. Returns the name to commit if this row's input just settled.
fn row(app: &mut PryHub, ui: &mut egui::Ui, r: &Row, g: Grid) -> Option<(u32, String)> {
    let t = app.lang.strings();
    // Reserved before the cells are drawn, so the hover wash lands *behind* them.
    let wash = ui.painter().add(Shape::Noop);
    let mut commit = None;
    let inner = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // The design sets the hash in the heaviest weight it has. There is no bold monospace in
        // this port, so the hash keeps the full ink and every other cell is mixed back instead.
        cell(ui, HASH_W, g.row_h, g.pad, |ui| {
            ui.add(
                Label::new(
                    RichText::new(format!("0x{:08X}", r.hash))
                        .font(theme::font::mono(14.0))
                        .color(token::TEXT),
                )
                .truncate(),
            );
        });

        cell(ui, g.name_w, g.row_h, g.pad, |ui| {
            // What the file itself says stands in as the placeholder: it is the name that would
            // apply if nothing were typed, and a hover says whether it survived the truncation.
            let (hint, whole) = match &r.file_name {
                Some(name) => (name.clone(), Some(gizmo_nfs::hash::string_hash(name) == r.hash)),
                None => (t.h_unknown.to_owned(), None),
            };
            let buf = app
                .dict
                .edits
                .entry(r.hash)
                .or_insert_with(|| app.names.get(r.hash).unwrap_or_default().to_string());
            let field_w = (g.name_w - g.pad * 2.0 - MARK - 6.0).max(80.0);
            let mut resp = ui.add_sized(
                [field_w, g.input_h],
                TextEdit::singleline(buf)
                    .font(theme::font::mono(12.0))
                    .margin(Margin::symmetric(10, 6))
                    .hint_text(hint),
            );
            if let Some(whole) = whole {
                resp = resp.on_hover_text(if whole { t.dc_whole } else { t.dc_truncated });
            }
            let typed = buf.clone();
            // Enter is scoped to the row that has the caret; unscoped, one keypress committed
            // every row on screen.
            let entered = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if resp.lost_focus() || entered {
                commit = Some((r.hash, typed.clone()));
            }

            ui.add_space(6.0);
            // The mark is the whole point of the screen, so it is *drawn*: a tick from a font the
            // user may not have would render as a box on the one glyph that has to be
            // unmistakable. A ring means "kept as a note", not "wrong".
            let (rect, mark) = ui.allocate_exact_size(Vec2::splat(MARK), Sense::hover());
            if !typed.trim().is_empty() {
                let ok = gizmo_nfs::hash::string_hash(typed.trim()) == r.hash;
                let (p, at) = (ui.painter(), rect.center());
                if ok {
                    theme::mark::ok(p, at, 12.0, token::ACCENT);
                } else {
                    theme::mark::unchecked(p, at, 12.0);
                }
                mark.on_hover_text(if ok { t.dc_verified_hint } else { t.dc_unverified_hint });
            }
        });

        // The design's `source` is where the *name* came from, not where the hash was seen. The
        // reference list the file gives is worth keeping, so it moves onto the chip's hover.
        cell(ui, SOURCE_W, g.row_h, g.pad, |ui| {
            let (label, tone) = if app.names.get(r.hash).is_some() {
                (t.h_src_user, widget::Tone::Accent)
            } else if r.file_name.is_some() {
                ("auto", widget::Tone::Neutral)
            } else {
                ("—", widget::Tone::Bare)
            };
            let chip = widget::tag(ui, label, tone);
            let refs = sources(r.sources, t);
            if !refs.is_empty() {
                chip.on_hover_text(refs);
            }
        });
    });

    let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), inner.response.rect.y_range());
    if ui.rect_contains_pointer(rect) {
        ui.painter().set(wash, Shape::rect_filled(rect, 0.0_f32, theme::muted(4)));
    }
    // `.table td { border-bottom: 1px solid }` — on every cell, which is what makes a list of
    // floating text lines read as one grid.
    theme::draw::rule_h(
        ui.painter(),
        rect.left()..=rect.right(),
        rect.bottom() - token::HAIRLINE,
        token::HAIRLINE,
        token::DIVIDER,
    );
    commit
}

/// The add row: a hash you already know but the open file never mentions, and the name for it.
///
/// Without it the only hashes that can be named are the ones some file happens to reference — and
/// a dictionary that can only be written while the right file is open is not a dictionary.
fn add_row(app: &mut PryHub, ui: &mut egui::Ui) {
    let t = app.lang.strings();
    let hash = parse_hash(&app.dict.new_hash);
    let clicked = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = token::SPACE_2;
            widget::input(ui, &mut app.dict.new_hash, 170.0, "0x________", true);
            // `flex:1` then a fixed button: laid out from the right, so the name field can take
            // whatever the button leaves.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let go = ui
                    .add_enabled_ui(hash.is_some(), |ui| {
                        widget::button_primary(ui, &format!("+ {}", t.h_add))
                    })
                    .inner;
                let rest = (ui.available_width() - token::SPACE_2).max(120.0);
                widget::input(ui, &mut app.dict.new_name, rest, t.h_name, true);
                go.clicked()
            })
            .inner
        })
        .inner;

    if !clicked {
        return;
    }
    let Some(hash) = hash else { return };
    let name = std::mem::take(&mut app.dict.new_name);
    app.dict.new_hash.clear();
    if !name.trim().is_empty() {
        app.names.set(hash, &name);
        save(app);
    }
    // A hash with no name yet is still worth a row: it is what candidates get typed into.
    if !app.dict.added.contains(&hash) {
        app.dict.added.push(hash);
    }
    app.dict.edits.insert(hash, name);
}

/// `0x8F3A21C0`, `8f3a21c0` — what the design's `0x________` invites, in either case and with or
/// without the prefix. `None` while what is typed is not yet a hash, which is what greys the button.
fn parse_hash(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    if digits.is_empty() {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// Write the dictionary and say where it went — the same answer whichever end of the screen the
/// name was typed at.
fn save(app: &mut PryHub) {
    let t = app.lang.strings();
    if let Some(result) = app.names.save_if_dirty() {
        app.dict.note = Some(match result {
            Ok(path) => format!("{} → {}", t.dc_saved, path.display()),
            Err(e) => format!("{}: {e}", t.dc_save_failed),
        });
    }
}

/// Where a hash was seen, as a compact list.
fn sources(s: Sources, t: &crate::i18n::Strings) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if s.texture {
        parts.push(t.dc_src_texture);
    }
    if s.material {
        parts.push(t.dc_src_material);
    }
    if s.shader {
        parts.push(t.dc_src_shader);
    }
    if s.part {
        parts.push(t.dc_src_part);
    }
    parts.join(" ")
}
