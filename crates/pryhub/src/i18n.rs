//! The interface in Turkish and English.
//!
//! The strings are the design's own table, carried over verbatim so the two stay comparable when
//! a screen is checked against the prototype. One struct with one field per string keeps the
//! compiler honest: a missing translation is a build error, not a blank label.

/// Which language the interface is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// Türkçe — the project's own working language, and the language its source comments are in.
    Tr,
    /// English. The default: someone who owns the game and finds this tool should not have their
    /// first window in a language they did not pick. The switch is in Settings.
    #[default]
    En,
}

impl Lang {
    /// The language's own name, for the settings menu. Never translated: a language is listed in
    /// itself, or the person who needs it cannot find it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Tr => "Türkçe",
            Self::En => "English",
        }
    }

    /// The string table for this language.
    #[must_use]
    pub fn strings(self) -> &'static Strings {
        match self {
            Self::Tr => &TR,
            Self::En => &EN,
        }
    }
}

/// A noun that is drawn after a number.
///
/// English agrees the noun with its count — `1 finding`, `2 findings` — and Turkish does not: a
/// numeral already carries the plural there, so `1 bulgu` and `4 bulgu` are both right. Both forms
/// are asked of *every* language rather than a rule being applied to one of them, because the
/// alternative is a branch that says "if English"; Turkish simply answers with the same word twice.
///
/// This is not pedantry about grammar. English became the default language of a fresh install, and
/// the first window someone saw said `1 warnings` in three places.
pub struct Counted {
    pub one: &'static str,
    pub many: &'static str,
}

impl Counted {
    /// The form that goes with `n`. Zero takes the plural, in both languages.
    #[must_use]
    pub fn of(&self, n: usize) -> &'static str {
        if n == 1 {
            self.one
        } else {
            self.many
        }
    }
}

/// A counted noun with two forms — the English table.
const fn counted(one: &'static str, many: &'static str) -> Counted {
    Counted { one, many }
}

/// A counted noun with one form, for a language that does not agree it with the numeral.
const fn same(word: &'static str) -> Counted {
    Counted { one: word, many: word }
}

/// Every piece of interface text, in one language.
///
/// Strings for screens that are designed but not yet drawn are carried too — they come from the
/// design's own table, and translating them once is cheaper than hunting them down later.
#[allow(dead_code)]
pub struct Strings {
    pub brand_sub: &'static str,
    pub m_open: &'static str,
    pub m_export: &'static str,
    /// The compare screen's second button: ask the desktop for a file, as opposed to `m_open`,
    /// which opens whatever the field beside it already holds.
    pub browse: &'static str,
    /// The settings menu.
    pub m_settings: &'static str,
    pub set_language: &'static str,
    pub set_size: &'static str,
    pub set_small: &'static str,
    pub set_medium: &'static str,
    pub set_large: &'static str,
    pub set_about: &'static str,
    pub set_help: &'static str,
    pub set_repo: &'static str,
    pub nav_workspace: &'static str,
    pub nav_validation: &'static str,
    pub nav_discovery: &'static str,
    pub nav_diff: &'static str,
    pub nav_dict: &'static str,
    /// The CARP screen. "CARP" in both languages: it is the format's own name, like `GEOMETRY.BIN`.
    pub nav_carp: &'static str,
    pub job_car_spec: &'static str,
    pub job_replace: &'static str,
    pub cp_not_chunk: &'static str,
    pub cp_raw: &'static str,
    pub cp_raw_sub: &'static str,
    pub cp_sections: &'static str,
    pub cp_levels: &'static str,
    pub cp_save: &'static str,
    pub cp_revert: &'static str,
    pub cp_no_changes: &'static str,
    pub cp_param: &'static str,
    pub cp_unit: &'static str,
    /// Drawn in a cell whose value this install does not carry.
    pub cp_not_located: &'static str,
    /// The panel note. The design promises the write path lands here; it cannot, so this says what
    /// is actually true instead.
    pub cp_write_note: &'static str,
    pub cp_no_install: &'static str,
    pub cp_no_record: &'static str,
    pub cp_from_globalb: &'static str,
    pub cp_located_of: &'static str,
    pub p_tree: &'static str,
    pub p_inspector: &'static str,
    pub p_log: &'static str,
    pub tab_3d: &'static str,
    pub tab_hex: &'static str,
    pub tab_tex: &'static str,
    /// The assembly tab: which parts are mounted, and the car they make.
    pub tab_asm: &'static str,
    pub asm_mounted: &'static str,
    pub asm_total: &'static str,
    pub asm_all: &'static str,
    pub asm_stock: &'static str,
    pub asm_hint: &'static str,
    /// The one part that cannot be taken off.
    pub asm_base: &'static str,
    pub asm_paint: &'static str,
    pub asm_paint_stock: &'static str,
    pub asm_full: &'static str,
    pub asm_partial: &'static str,
    pub asm_tri: Counted,
    pub matrix: &'static str,
    /// The chips the inspector puts on a field: read straight out of the file, worked out from it,
    /// or not believed. "Suspect" rather than "error" — the rule flags it, the reader judges it.
    pub f_derived: &'static str,
    pub f_const: &'static str,
    pub f_suspect: &'static str,
    /// Over the 3D viewport: the part's bounding box, and how to move the camera.
    pub bbox: &'static str,
    pub drag_hint: &'static str,
    /// Shown when the viewport is showing the showroom car rather than one part.
    pub stock_car: &'static str,
    /// The viewport's own buttons.
    pub wire: &'static str,
    pub rotate: &'static str,
    pub reset: &'static str,
    /// Stands in for the whole viewport when there is no adapter. Not in the design — the mock
    /// always had a canvas, so this is the one string the app needs and the prototype never did.
    pub no_gpu: &'static str,
    /// The hex view's legend: which colour is the chunk's own extent, which is a field inside it.
    pub hx_region: &'static str,
    pub hx_field: &'static str,
    /// The texture tab: the empty states, and the counts over the contact sheet.
    pub no_textures: &'static str,
    /// Shown while the worker is decoding a pack, instead of a frozen window.
    pub decoding: &'static str,
    pub pick_texture: &'static str,
    pub textures_count: Counted,
    pub textures_undecoded: &'static str,
    /// Distinct from [`Strings::textures_undecoded`], and the distinction is the whole point: these
    /// were never attempted, because the pack is larger than one document is allowed to hold. Saying
    /// "could not be decoded" of them would blame the file for a limit this program set.
    pub textures_unread: &'static str,
    /// The row labels under a single texture's preview. "hash" is the same word in both languages
    /// — it is what the file stores, not a description of it.
    pub tx_hash: &'static str,
    pub tx_size: &'static str,
    pub tx_format: &'static str,
    pub tx_opaque: &'static str,
    /// Export: the toolbar button's hover text, and the two log lines it can produce.
    pub export_hint: &'static str,
    pub exported: &'static str,
    pub export_failed: &'static str,
    /// The units an export's summary line counts in.
    pub ex_parts: Counted,
    pub ex_materials: Counted,
    /// Written out rather than a `▲`: the bundled font has no triangle, and a missing glyph is a box.
    pub ex_triangles: Counted,
    /// The export dialog. glTF, OBJ, GLB, PNG and DDS are names of formats, not words, so they
    /// stay out of the table.
    pub exp_title: &'static str,
    pub exp_scope: &'static str,
    pub exp_scope_selection: &'static str,
    pub exp_scope_all: &'static str,
    /// The third scope: what the assembly tab has mounted.
    pub exp_scope_build: &'static str,
    pub exp_model: &'static str,
    pub exp_tex: &'static str,
    pub exp_both: &'static str,
    pub exp_target: &'static str,
    pub exp_cancel: &'static str,
    pub exp_go: &'static str,
    /// States the one difference that decides the format, so the choice needs no tooltip.
    pub exp_note: &'static str,
    /// Why a row the design draws as a choice cannot be chosen from. Both are about this tool's
    /// limits rather than about the file, so they say which limit rather than "unavailable".
    pub exp_png_only: &'static str,
    /// Replacing a texture — the first thing this tool writes into a game file rather than beside
    /// one, so its words are about what will be written and where, not about the action.
    pub rep_action: &'static str,
    pub rep_hint: &'static str,
    pub rep_title: &'static str,
    pub rep_png: &'static str,
    /// The save dialog's list of what is staged, and the chip that counts it on the sheet.
    pub rep_list: &'static str,
    /// The sheet's own Save. **Not** CARP's `cp_save`, which reads "Save CARP" — borrowing that
    /// screen's treatment is right and borrowing its wording is not, because it names a screen this
    /// button has nothing to do with.
    pub rep_save: &'static str,
    pub rep_changes: Counted,
    pub rep_target: &'static str,
    pub rep_copy: &'static str,
    pub rep_over: &'static str,
    pub rep_go: &'static str,
    /// The rule a replacement has to keep, stated where the choice is made rather than reported
    /// afterwards as an error.
    pub rep_note: &'static str,
    /// What overwriting does before it overwrites.
    pub rep_over_note: &'static str,
    /// The log's two lines, and the two things a written pack can have to say about itself.
    pub replaced: &'static str,
    pub replace_failed: &'static str,
    pub rep_moved: &'static str,
    pub rep_exact: &'static str,
    pub no_parts: &'static str,
    /// The log's own vocabulary. A note carries what happened; these are the words for it.
    pub n_decompressed: &'static str,
    pub n_decompress_failed: &'static str,
    pub n_tree_failed: &'static str,
    pub n_no_part: &'static str,
    pub n_geometry_failed: &'static str,
    /// Why a solid yielded no part, in the interface's words.
    pub skip_not_a_mesh: &'static str,
    pub skip_empty: &'static str,
    pub skip_packed: &'static str,
    pub skip_unknown: &'static str,
    /// The discovery screen: the schema controls, and the table's own vocabulary.
    pub d_header: &'static str,
    pub d_stride: &'static str,
    pub d_records: &'static str,
    pub d_left: &'static str,
    pub d_unclaimed: &'static str,
    pub d_candidates: &'static str,
    pub d_candidate_hint: &'static str,
    pub d_guess: &'static str,
    pub d_guess_hint: &'static str,
    pub d_offset: &'static str,
    pub d_cycle: &'static str,
    pub d_add: &'static str,
    pub d_remove: &'static str,
    /// What the screen is and what it is called in the mode list. The tag is loud on purpose:
    /// this is the one mode the Windows tools have no answer to.
    pub d_sub: &'static str,
    pub d_pattern: &'static str,
    pub d_add_field: &'static str,
    /// The hint in a column's name box, before anyone has called it anything.
    pub d_name: &'static str,
    pub d_preview: &'static str,
    pub d_hint: &'static str,
    pub d_tag: &'static str,
    /// The two units a stride is read in: so many bytes make a row, so many rows come out.
    pub d_rows: Counted,
    pub d_bytes_row: &'static str,
    /// The compare screen.
    pub df_left: &'static str,
    pub df_right: &'static str,
    pub df_pick: &'static str,
    pub df_identical: &'static str,
    pub df_changed: &'static str,
    pub df_resized: &'static str,
    pub df_only_left: &'static str,
    pub df_only_right: &'static str,
    pub df_same: &'static str,
    pub df_all: &'static str,
    pub df_first: &'static str,
    pub df_go: &'static str,
    /// The dictionary screen.
    pub dc_hint: &'static str,
    /// The dictionary's own meta line counts rows, and English wants a plural on the word the
    /// column header leaves alone — `hash` is the design's literal there, `4 hashes` is a count.
    pub dc_hashes: Counted,
    pub dc_named: &'static str,
    pub dc_in_dictionary: &'static str,
    pub dc_verified: &'static str,
    pub dc_verified_hint: &'static str,
    pub dc_unverified_hint: &'static str,
    pub dc_filter: &'static str,
    pub dc_your_name: &'static str,
    pub dc_whole: &'static str,
    pub dc_truncated: &'static str,
    pub dc_saved: &'static str,
    pub dc_save_failed: &'static str,
    pub dc_src_texture: &'static str,
    pub dc_src_material: &'static str,
    pub dc_src_shader: &'static str,
    pub dc_src_part: &'static str,
    /// The screen's subtitle and its table's columns. The hash column is not named — the numbers
    /// under it say what they are.
    pub h_sub: &'static str,
    pub h_name: &'static str,
    pub h_source: &'static str,
    pub h_unknown: &'static str,
    pub h_add: &'static str,
    /// Only a name someone typed is translated; `auto` is what the code wrote and stays as it is.
    pub h_src_user: &'static str,
    /// What the worker is doing, for the status bar. The program's own log says the same three
    /// things in English (`jobs::Kind::as_str`) — this is the half a person reads.
    pub job_open: &'static str,
    pub job_decode: &'static str,
    /// Reading a pack's names without its pixels — the dictionary's job, not the texture tab's.
    pub job_names: &'static str,
    pub job_export: &'static str,
    pub job_palette: &'static str,
    pub st_chunks: Counted,
    pub st_sel: &'static str,
    pub st_scale: &'static str,
    pub log_all: &'static str,
    pub log_warn: &'static str,
    pub log_err: &'static str,
    pub log_info: &'static str,
    pub w_open_source: &'static str,
    pub w_validates: &'static str,
    pub w_discovers: &'static str,
    pub w_open_file: &'static str,
    pub w_drop: &'static str,
    pub w_recent: &'static str,
    pub w_modes: &'static str,
    pub w_card_validation: &'static str,
    pub w_card_discovery: &'static str,
    pub w_card_diff: &'static str,
    pub w_card_dict: &'static str,
    /// Under the four cards. It names the four axes instead of claiming a niche, because the
    /// tools this one is measured against do not run here at all.
    pub w_strategy: &'static str,
    /// Shown in the centre area before a file is open.
    pub no_file: &'static str,
    pub open_failed: &'static str,
    pub nothing_selected: &'static str,
    pub val_no_findings: &'static str,
    /// How many chunks a rule read, and how many findings it produced.
    pub val_examined: Counted,
    pub val_findings: Counted,
    /// Shown for a rule that had nothing to read — deliberately not phrased as a pass.
    pub val_unread: &'static str,
    /// The tallies over the screen, and the three words that say when it runs: on open, unasked.
    pub v_warns: Counted,
    pub v_pass: &'static str,
    pub v_auto: &'static str,
}

static TR: Strings = Strings {
    brand_sub: "nfsu2 varlık aracı",
    m_open: "Aç",
    m_export: "Dışa Aktar",
    browse: "Gözat",
    m_settings: "Ayarlar",
    set_language: "DİL",
    set_size: "BOYUT",
    set_small: "Küçük",
    set_medium: "Orta",
    set_large: "Büyük",
    set_about: "HAKKINDA",
    set_help: "Yardım (README)",
    set_repo: "GitHub deposu",
    nav_workspace: "Çalışma",
    nav_validation: "Doğrulama",
    nav_discovery: "Keşif",
    nav_diff: "Karşılaştır",
    nav_dict: "Sözlük",
    nav_carp: "CARP",
    job_car_spec: "araç kaydı okunuyor",
    job_replace: "doku yazılıyor",
    cp_not_chunk: "chunk formatı değil — ayrı parser",
    cp_raw: "HAM CARP",
    cp_raw_sub: "tüm bölümler ve parametreler",
    cp_sections: "BÖLÜMLER",
    cp_levels: "Yükseltme seviyesi",
    cp_save: "CARP'ı kaydet",
    cp_revert: "Geri al",
    cp_no_changes: "değişiklik yok",
    cp_param: "parametre",
    cp_unit: "birim",
    cp_not_located: "bu oyunun dosyalarında yok",
    cp_write_note: "NFSU2 bir CARP.BIN göndermiyor; veri GLOBALB.BUN'daki 0x00034600 kaydında \
(46 × 2192 bayt). Motor, tork eğrisi ve vites kutusu oradan okunuyor — vites kutusu dört \
yükseltme sütununun tamamıyla, çünkü dosyada dört kez saklanan tek şey o. Tork noktalarının \
devirleri dosyada yazmıyor: rölantiden devir kesiciye sekiz eşit adım, oyunun kendi \
dinamometresine karşı doğrulandı. Aero ve fren bu oyunun dosyalarında yok; direksiyondan yalnızca \
çarpan var, o da sürülerek doğrulandı.",
    cp_no_install: "Kurulum bulunamadı — GLOBALB.BUN okunamadı",
    cp_no_record: "GLOBALB.BUN'da bu ada sahip araç kaydı yok",
    cp_from_globalb: "GLOBALB",
    cp_located_of: "parametrenin karşılığı bulundu",
    p_tree: "AĞAÇ",
    p_inspector: "DENETÇİ",
    p_log: "GÜNLÜK",
    tab_3d: "3D",
    tab_hex: "HEX",
    tab_tex: "DOKU",
    tab_asm: "MONTAJ",
    asm_mounted: "TAKILI PARÇALAR",
    asm_total: "toplam",
    asm_all: "Hepsi",
    asm_stock: "Standart",
    asm_hint: "bir parçayı aç/kapa — yapı anında güncellenir",
    asm_base: "temel",
    asm_paint: "BOYA",
    asm_paint_stock: "standart",
    asm_full: "Tam yapı — bütün parçalar takılı",
    asm_partial: "parça çıkarıldı",
    asm_tri: same("üçgen"),
    matrix: "Birim matris (4×4)",
    f_derived: "türetildi",
    f_const: "sabit",
    f_suspect: "şüpheli",
    bbox: "bbox",
    drag_hint: "sürükle döndür · kaydır yakınlaş",
    stock_car: "stok araç",
    wire: "Tel kafes",
    rotate: "Döndür",
    reset: "Sıfırla",
    no_gpu: "wgpu arka ucu yok",
    hx_region: "chunk bölgesi",
    hx_field: "sayaç alanı",
    no_textures: "Bu dosyada doku yok — yanında TEXTURES.BIN de bulunamadı",
    decoding: "Dokular çözülüyor…",
    pick_texture: "Izgaradan bir doku seç",
    textures_count: same("doku"),
    textures_undecoded: "çözülemedi",
    textures_unread: "okunmadı (paket sınırı)",
    tx_hash: "hash",
    tx_size: "boyut",
    tx_format: "biçim",
    tx_opaque: "opaklık",
    export_hint: "ekranda ne varsa onu pryhub-export/ altına yazar",
    exported: "yazıldı",
    export_failed: "dışa aktarılamadı",
    ex_parts: same("parça"),
    ex_materials: same("materyal"),
    ex_triangles: same("üçgen"),
    exp_title: "Dışa Aktar",
    exp_scope: "Kapsam",
    exp_scope_selection: "Seçili parça",
    exp_scope_all: "Tüm dosya",
    exp_scope_build: "Montaj",
    exp_model: "Model biçimi",
    exp_tex: "Doku biçimi",
    exp_both: "İkisi",
    exp_target: "Hedef klasör",
    exp_cancel: "Vazgeç",
    exp_go: "Aktar",
    exp_note: "glTF materyal + hiyerarşiyi taşır — OBJ taşımaz.",
    exp_png_only: "çözülen tek doku biçimi PNG",
    rep_action: "Değiştir…",
    rep_hint: "seçili dokunun yerine bir PNG koyar",
    rep_title: "Değişiklikleri yaz",
    rep_png: "PNG dosyası",
    rep_list: "Yazılacaklar",
    rep_save: "Kaydet",
    rep_changes: same("değişiklik"),
    rep_target: "Nereye yazılsın",
    rep_copy: "Kopyasına",
    rep_over: "Dosyanın üzerine",
    rep_go: "Yaz",
    rep_note: "Görüntü dokunun kendi ölçüsünde olmalı: en ile boy blob'un gömülü başlığında yazılı ve tanımlayıcı o başlığı sondan sayarak buluyor. Biçim de dokunun kendi biçimi kalır.",
    rep_over_note: "önce yanına .bak yazılır",
    replaced: "değiştirildi",
    replace_failed: "değiştirilemedi",
    rep_moved: "paket yeniden yerleştirildi",
    rep_exact: "birebir geri okundu",
    no_parts: "bu dosyada parça yok",
    n_decompressed: "açıldı",
    n_decompress_failed: "açılamadı — ham baytlar okunuyor",
    n_tree_failed: "chunk ağacı okunamadı",
    n_no_part: "parça yok",
    n_geometry_failed: "geometri okunamadı",
    skip_not_a_mesh: "mesh chunk'ı yok (bağlantı noktası)",
    skip_empty: "sayaçlar sıfır",
    skip_packed: "paketli vertex düzeni",
    skip_unknown: "bilinmeyen",
    d_header: "başlık",
    d_stride: "stride",
    d_records: "kayıt",
    d_left: "artıyor",
    d_unclaimed: "sahipsiz",
    d_candidates: "tam bölen:",
    d_candidate_hint: "stride × kayıt — payload'ı kalansız bölenler",
    d_guess: "tahmin et",
    d_guess_hint: "her 4 baytlık şeridi örnekleyip tipini öner",
    d_offset: "offset",
    d_cycle: "tıkla: tipi değiştir",
    d_add: "sütun ekle",
    d_remove: "son sütunu sil",
    d_sub: "Bilinmeyen chunk’ı seç, alan/stride tanımla, tablo anında yenilensin — gömülü ImHex.",
    d_pattern: "Desen",
    d_add_field: "Alan ekle",
    d_name: "alan adı",
    d_preview: "Canlı önizleme",
    d_hint: "İlke: hiçbir offset’i sabit tutma. Her şey stride ve sayaçlardan türesin.",
    d_tag: "EN ÇOK AYRIŞTIRAN",
    d_rows: same("satır"),
    d_bytes_row: "bayt/satır",
    df_left: "A dosyası",
    df_right: "B dosyası",
    df_pick: "Karşılaştırmak için ikinci dosyayı seç — yolu yaz ya da pencereye bırak",
    df_identical: "İki dosya birebir aynı: aynı chunk'lar, aynı boyutlar, aynı baytlar",
    df_changed: "değişti",
    df_resized: "boyut değişti",
    df_only_left: "yalnız solda",
    df_only_right: "yalnız sağda",
    df_same: "aynı",
    df_all: "aynı olanları da listele",
    df_first: "ilk fark",
    df_go: "tıkla: sol dosyada bu chunk'a git",
    dc_hint: "Dosyanın işaret ettiği her hash. Verdiğin isim hash'e geri dönüyorsa işaretlenir — kırpılmış bir ismin kuyruğu ancak böyle geri gelir.",
    dc_hashes: same("hash"),
    dc_named: "isimli",
    dc_in_dictionary: "sözlükte",
    dc_verified: "doğrulandı",
    dc_verified_hint: "bu isim dosyadaki sayıyı üretiyor — kanıt",
    dc_unverified_hint: "bu isim sayıyı üretmiyor — not olarak tutuluyor",
    dc_filter: "hash ya da isim süz",
    dc_your_name: "isim ver",
    dc_whole: "dosyadaki isim tam: hash'e geri dönüyor",
    dc_truncated: "dosyadaki isim kırpık: hash'e geri dönmüyor",
    dc_saved: "sözlük yazıldı",
    dc_save_failed: "sözlük yazılamadı",
    dc_src_texture: "doku",
    dc_src_material: "materyal",
    dc_src_shader: "shader",
    dc_src_part: "parça",
    h_sub: "Dokular yalnızca hash tutuyor — isim eşlemesi burada.",
    h_name: "isim",
    h_source: "kaynak",
    h_unknown: "bilinmiyor",
    h_add: "Ekle",
    h_src_user: "kullanıcı",
    job_open: "açılıyor",
    job_decode: "çözülüyor",
    job_names: "isimler okunuyor",
    job_export: "dışa aktarılıyor",
    job_palette: "palet okunuyor",
    st_chunks: same("chunk"),
    st_sel: "seçim",
    st_scale: "1u = 1m · Z↑",
    log_all: "Tümü",
    log_warn: "Uyarı",
    log_err: "Hata",
    log_info: "Bilgi",
    w_open_source: "açık kaynak",
    w_validates: "doğrular",
    w_discovers: "keşfettirir",
    w_open_file: "Dosya aç",
    w_drop: "Dosya bırak ya da tıkla",
    w_recent: "Son kullanılanlar",
    w_modes: "Modlar",
    w_card_validation: "Aç, gez, gör, uyar. Otomatik sağlık kontrolü.",
    w_card_discovery: "Bilinmeyen chunk'ı canlı stride/alanla çöz.",
    w_card_diff: "İki dosyayı yan yana, chunk farkları.",
    w_card_dict: "Doku hash'lerine kendi isimlerini ver.",
    w_strategy: "Konumlandırma: taklit değil, dört eksende aşmak — Linux-native + açık kaynak + doğrulayan + keşfettiren.",
    no_file: "Bir dosya aç",
    open_failed: "Dosya açılamadı",
    nothing_selected: "Ağaçtan bir chunk seç",
    val_no_findings: "Bulgu yok — dosya temiz görünüyor",
    val_examined: same("chunk okundu"),
    val_findings: same("bulgu"),
    val_unread: "bu dosyada okunacak bir şey yoktu — geçti demek değil",
    v_warns: same("uyarı"),
    v_pass: "geçti",
    v_auto: "otomatik sağlık kontrolü",
};

static EN: Strings = Strings {
    brand_sub: "nfsu2 asset toolkit",
    m_open: "Open",
    m_export: "Export",
    browse: "Browse",
    m_settings: "Settings",
    set_language: "LANGUAGE",
    set_size: "SIZE",
    set_small: "Small",
    set_medium: "Medium",
    set_large: "Large",
    set_about: "ABOUT",
    set_help: "Help (README)",
    set_repo: "GitHub repository",
    nav_workspace: "Workspace",
    nav_validation: "Validation",
    nav_discovery: "Discovery",
    nav_diff: "Compare",
    nav_dict: "Dictionary",
    nav_carp: "CARP",
    job_car_spec: "reading the car record",
    job_replace: "writing a texture",
    cp_not_chunk: "not a chunk format — separate parser",
    cp_raw: "RAW CARP",
    cp_raw_sub: "all sections and params",
    cp_sections: "SECTIONS",
    cp_levels: "Upgrade level",
    cp_save: "Save CARP",
    cp_revert: "Revert",
    cp_no_changes: "no changes",
    cp_param: "param",
    cp_unit: "unit",
    cp_not_located: "not in this game's files",
    cp_write_note: "NFSU2 ships no CARP.BIN; the data is in GLOBALB.BUN's 0x00034600 record \
(46 × 2192 bytes). Engine limits, the torque curve and the gearbox are read from it — the gearbox \
across all four upgrade columns, since it is the only thing the record stores four times. The rpm \
each torque point sits at is not stored: it is idle to limiter in eight equal steps, checked \
against the game's own dynamometer. Aero and brakes are not in this game's files; of the steering \
only the multiplier is, and that one was settled by driving it.",
    cp_no_install: "No install found — GLOBALB.BUN was not readable",
    cp_no_record: "GLOBALB.BUN holds no car record under this name",
    cp_from_globalb: "GLOBALB",
    cp_located_of: "params have a value here",
    p_tree: "TREE",
    p_inspector: "INSPECTOR",
    p_log: "LOG",
    tab_3d: "3D",
    tab_hex: "HEX",
    tab_tex: "TEXTURE",
    tab_asm: "ASSEMBLY",
    asm_mounted: "MOUNTED PARTS",
    asm_total: "total",
    asm_all: "All",
    asm_stock: "Stock",
    asm_hint: "toggle a part — the build updates live",
    asm_base: "base",
    asm_paint: "PAINT",
    asm_paint_stock: "stock",
    asm_full: "Final build — all parts mounted",
    asm_partial: "removed",
    asm_tri: counted("tri", "tri"),
    matrix: "Unit matrix (4×4)",
    f_derived: "derived",
    f_const: "const",
    f_suspect: "suspect",
    bbox: "bbox",
    drag_hint: "drag to orbit · scroll to zoom",
    stock_car: "showroom car",
    wire: "Wireframe",
    rotate: "Rotate",
    reset: "Reset",
    no_gpu: "no wgpu backend",
    hx_region: "chunk region",
    hx_field: "counter field",
    no_textures: "No textures in this file — and no TEXTURES.BIN beside it",
    decoding: "Decoding textures…",
    pick_texture: "Pick a texture from the grid",
    textures_count: counted("texture", "textures"),
    textures_undecoded: "could not be decoded",
    textures_unread: "not read (pack limit)",
    tx_hash: "hash",
    tx_size: "size",
    tx_format: "format",
    tx_opaque: "opaque",
    export_hint: "writes whatever is on screen under pryhub-export/",
    exported: "written",
    export_failed: "export failed",
    ex_parts: counted("part", "parts"),
    ex_materials: counted("material", "materials"),
    ex_triangles: counted("triangle", "triangles"),
    exp_title: "Export",
    exp_scope: "Scope",
    exp_scope_selection: "Selection",
    exp_scope_all: "Whole file",
    exp_scope_build: "Build",
    exp_model: "Model format",
    exp_tex: "Texture format",
    exp_both: "Both",
    exp_target: "Target folder",
    exp_cancel: "Cancel",
    exp_go: "Export",
    exp_note: "glTF carries materials + hierarchy — OBJ does not.",
    exp_png_only: "PNG is the only decoded texture format",
    rep_action: "Replace…",
    rep_hint: "put a PNG in place of the selected texture",
    rep_title: "Write changes",
    rep_png: "PNG file",
    rep_list: "What goes in",
    rep_save: "Save",
    rep_changes: counted("change", "changes"),
    rep_target: "Where it goes",
    rep_copy: "To a copy",
    rep_over: "Over the original",
    rep_go: "Write",
    rep_note: "The image must have the texture's own dimensions: width and height live in the blob's embedded header, which the descriptor finds by counting back from the end. The format stays the texture's own too.",
    rep_over_note: "a .bak is written beside it first",
    replaced: "replaced",
    replace_failed: "could not be replaced",
    rep_moved: "the pack was relocated",
    rep_exact: "read back identical",
    no_parts: "this file holds no parts",
    n_decompressed: "decompressed",
    n_decompress_failed: "could not decompress — reading the raw bytes",
    n_tree_failed: "could not read the chunk tree",
    n_no_part: "no part",
    n_geometry_failed: "could not read the geometry",
    skip_not_a_mesh: "no mesh chunk (a mount point)",
    skip_empty: "counters are zero",
    skip_packed: "packed vertex layout",
    skip_unknown: "unknown",
    d_header: "header",
    d_stride: "stride",
    d_records: "records",
    d_left: "left over",
    d_unclaimed: "unclaimed",
    d_candidates: "divides exactly:",
    d_candidate_hint: "stride × records — the strides that leave no remainder",
    d_guess: "guess",
    d_guess_hint: "sample every 4-byte lane and propose its type",
    d_offset: "offset",
    d_cycle: "click to change the type",
    d_add: "add a column",
    d_remove: "drop the last column",
    d_sub: "Pick an unknown chunk, define fields/stride, table refreshes live — embedded ImHex.",
    d_pattern: "Pattern",
    d_add_field: "Add field",
    d_name: "field name",
    d_preview: "Live preview",
    d_hint: "Principle: never hardcode an offset. Everything derives from stride and counters.",
    d_tag: "MOST DIFFERENTIATING",
    d_rows: counted("row", "rows"),
    d_bytes_row: "bytes/row",
    df_left: "File A",
    df_right: "File B",
    df_pick: "Pick a second file to compare — type a path, or drop one on the window",
    df_identical: "The two files are identical: same chunks, same sizes, same bytes",
    df_changed: "changed",
    df_resized: "resized",
    df_only_left: "only left",
    df_only_right: "only right",
    df_same: "same",
    df_all: "list identical chunks too",
    df_first: "first difference",
    df_go: "click to go to this chunk in the left file",
    dc_hint: "Every hash the file points at. A name that hashes back to the number gets a tick — the only way a truncated name's tail comes back.",
    dc_hashes: counted("hash", "hashes"),
    dc_named: "named",
    dc_in_dictionary: "in the dictionary",
    dc_verified: "verified",
    dc_verified_hint: "this name reproduces the file's number — proof",
    dc_unverified_hint: "this name does not reproduce the number — kept as a note",
    dc_filter: "filter by hash or name",
    dc_your_name: "name it",
    dc_whole: "the file's name is whole: it hashes back",
    dc_truncated: "the file's name is truncated: it does not hash back",
    dc_saved: "dictionary written",
    dc_save_failed: "could not write the dictionary",
    dc_src_texture: "texture",
    dc_src_material: "material",
    dc_src_shader: "shader",
    dc_src_part: "part",
    h_sub: "Textures store only a hash — name mapping lives here.",
    h_name: "name",
    h_source: "source",
    h_unknown: "unknown",
    h_add: "Add",
    h_src_user: "user",
    job_open: "opening",
    job_decode: "decoding",
    job_names: "reading names",
    job_export: "exporting",
    job_palette: "reading palette",
    st_chunks: counted("chunk", "chunks"),
    st_sel: "sel",
    st_scale: "1u = 1m · Z↑",
    log_all: "All",
    log_warn: "Warn",
    log_err: "Error",
    log_info: "Info",
    w_open_source: "open source",
    w_validates: "validates",
    w_discovers: "discovers",
    w_open_file: "Open a file",
    w_drop: "Drop a file or click",
    w_recent: "Recent",
    w_modes: "Modes",
    w_card_validation: "Open, browse, see, warn. Automatic health check.",
    w_card_discovery: "Solve unknown chunks with live stride/fields.",
    w_card_diff: "Two files side by side, chunk diff.",
    w_card_dict: "Name texture hashes yourself.",
    w_strategy: "Positioning: not imitation but surpassing on four axes — Linux-native + open source + validating + discovering.",
    no_file: "Open a file",
    open_failed: "Could not open the file",
    nothing_selected: "Select a chunk in the tree",
    val_no_findings: "No findings — the file looks clean",
    val_examined: counted("chunk read", "chunks read"),
    val_findings: counted("finding", "findings"),
    val_unread: "nothing here for this rule to read — which is not a pass",
    v_warns: counted("warning", "warnings"),
    v_pass: "passed",
    v_auto: "automatic health check",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_are_populated_and_distinct() {
        // A copy-paste slip would leave an English string in the Turkish table; the panel captions
        // are the ones most likely to be forgotten.
        assert_eq!(Lang::Tr.strings().p_tree, "AĞAÇ");
        assert_eq!(Lang::En.strings().p_tree, "TREE");
        assert_ne!(Lang::Tr.strings().nav_validation, Lang::En.strings().nav_validation);
    }

    /// A language is listed in itself, so someone who reads only one of the two can find it.
    #[test]
    fn each_language_is_named_in_its_own_language() {
        assert_eq!(Lang::Tr.name(), "Türkçe");
        assert_eq!(Lang::En.name(), "English");
    }

    /// The default the settings panel exists to establish.
    #[test]
    fn the_default_language_is_english() {
        assert_eq!(Lang::default(), Lang::En);
    }

    /// English agrees a noun with its number and Turkish does not. The bug this pins down is an
    /// English window — the default one — saying `1 warnings`, which it did in three places.
    #[test]
    fn a_counted_noun_agrees_in_english_and_stands_still_in_turkish() {
        let en = Lang::En.strings();
        assert_eq!(en.val_findings.of(1), "finding");
        assert_eq!(en.val_findings.of(2), "findings");
        // Zero takes the plural: "0 findings", not "0 finding".
        assert_eq!(en.val_findings.of(0), "findings");

        let tr = Lang::Tr.strings();
        assert_eq!(tr.val_findings.of(1), tr.val_findings.of(4));
    }
}
