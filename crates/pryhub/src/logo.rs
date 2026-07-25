//! Projenin markası: tek bir PNG, ikili dosyanın içine gömülü.
//!
//! Binary'nin yanında duran bir dosya değil `include_bytes!`, çünkü bu araç tek bir çalıştırılabilir
//! olarak taşınıyor — yanındaki klasörü kaybedince markasını da kaybeden bir program olmasın.
//! 499 × 159'luk asıl çözünürlük korunuyor: egui'nin kendi ölçekleyicisi küçültmeyi iyi yapıyor,
//! büyütmeyi yapamıyor, ve üst bardaki 22 px ile karşılama ekranındaki 56 px arasında üç kat var.
//!
//! Rengi olduğu gibi: marka mavisi (#0387f3 / #055cac), tasarımın kırmızı vurgusunun yanında.
//! Bu bir tercih, bir kaza değil — logo tasarım sisteminden önce vardı.

use egui::{Color32, Response, Sense, TextureHandle, Ui, Vec2};

/// Ham dosya. Tek kopya, tek gerçek.
const BYTES: &[u8] = include_bytes!("../assets/logo.png");

/// Yüklenmiş marka, çizilmeye hazır.
pub struct Logo {
    texture: TextureHandle,
    /// Genişlik / yükseklik — çağıran yalnızca yükseklik verir, genişliği bu söyler.
    aspect: f32,
}

impl Logo {
    /// PNG'yi çöz ve GPU'ya bir kez yükle.
    ///
    /// `None` dönmesi ölümcül değil: marka çizilmez, arayüz kelime markasıyla devam eder. Bir
    /// logonun çözülememesi, bir dosya tarayıcısının açılmaması için yeterli bir sebep değil.
    #[must_use]
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        let decoder = png::Decoder::new(BYTES);
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0_u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        // Dosya RGBA8 olarak yazıldı; başka bir şeyle karşılaşırsak çizmemek, yanlış çizmekten iyi.
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            log::warn!(target: "logo", "unexpected PNG format: {:?}/{:?}", info.color_type, info.bit_depth);
            return None;
        }
        let (w, h) = (info.width as usize, info.height as usize);
        let pixels: Vec<Color32> = buf[..w * h * 4]
            .chunks_exact(4)
            .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        let image = egui::ColorImage { size: [w, h], pixels, source_size: Vec2::new(w as f32, h as f32) };
        // Doğrusal filtre: mark her iki yerde de küçültülerek çiziliyor, ve bir eğrinin kenarı
        // nearest ile merdivene döner — doku sekmesinin tersi bir gereksinim, orada texel görmek
        // istiyoruz, burada görmek istemiyoruz.
        Some(Self {
            texture: ctx.load_texture("pryhub.logo", image, egui::TextureOptions::LINEAR),
            aspect: w as f32 / h as f32,
        })
    }

    /// Çizmeye yetecek kadarı, kopyalanabilir halde.
    ///
    /// Üst bar `&mut PryHub` tutarken çiziliyor, yani `self.logo`'ya ödünç bir referans o kapanışın
    /// içinde yaşayamaz. İki `Copy` alan dışarı alınınca ödünç orada biter ve bar kendi durumunu
    /// yazmaya devam edebilir.
    #[must_use]
    pub fn mark(&self) -> Mark {
        Mark { id: self.texture.id(), aspect: self.aspect }
    }
}

/// Markanın çizilebilir tutamağı.
#[derive(Clone, Copy)]
pub struct Mark {
    id: egui::TextureId,
    aspect: f32,
}

impl Mark {
    /// Verilen yükseklikte ne kadar yer kaplar.
    #[must_use]
    pub fn width_for(self, height: f32) -> f32 {
        height * self.aspect
    }

    /// Yer ayırarak çiz — akış içinde duran her yer için.
    pub fn show(self, ui: &mut Ui, height: f32) -> Response {
        let size = Vec2::new(self.width_for(height), height);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        self.paint(ui.painter(), rect);
        resp
    }

    /// Verilen dikdörtgene çiz — yerleşimi çağıranın kendi hesapladığı yerler için.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect) {
        painter.image(
            self.id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
