//! Masaüstünün kendi dosya seçicisi, alt süreç olarak.
//!
//! `Cargo.toml` `rfd`'yi reddediyor ve gerekçesi hâlâ geçerli: Linux'ta ya GTK geliştirme
//! kütüphanelerini ya da xdg-portal + tokio yığınını bu ikili dosyanın içine sokuyor — tek bir
//! düğme için ağır bir bedel. Ama karşılama ekranı "**ya da tıkla**" diyor ve tıklayınca hiçbir şey
//! olmuyordu; bir arayüzün yapmadığı şeyi vaat etmesi, seçiciyi hiç sunmamasından kötü.
//!
//! Ortadaki yol: masaüstünde zaten kurulu olan seçiciyi çalıştır. Derleme bağımlılığı yok, bağlanan
//! bir kütüphane yok, ve kullanıcı her zamanki penceresini görüyor. Hiçbiri yoksa bu modül bunu
//! *söylüyor* (`available()`), arayüz de tıklamayı yol alanına odaklanmaya çeviriyor — sessizce
//! hiçbir şey yapmak yerine.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// Denenecek seçiciler, sırayla. İlk bulunan kazanır.
const CHOOSERS: &[&str] = &["zenity", "kdialog", "qarma", "yad"];

/// Seçicinin ne arayacağı.
///
/// Süzgeç bir kolaylık değil: bu pencere iki ayrı şey için açılıyor ve ikisi birbirinin dosyasını
/// kabul etmiyor. Oyunun kabını PNG diye vermek de, PNG'yi belge diye açmaya çalışmak da sessizce
/// yanlış sonuç veriyor — hangisi istendiği burada söyleniyor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    /// Oyunun kapları — açılacak dosya.
    Assets,
    /// Bir dokunun yerine konacak görüntü.
    Images,
}

impl Filter {
    /// `desenler|etiket` — kdialog'un dilbilgisi.
    fn kdialog(self) -> &'static str {
        match self {
            Self::Assets => "*.BIN *.bin *.VIV *.viv *.BUN *.bun|NFSU2",
            Self::Images => "*.png *.PNG|PNG",
        }
    }

    /// `etiket | desenler` — zenity ailesinin dilbilgisi.
    fn gtk(self) -> &'static str {
        match self {
            Self::Assets => "--file-filter=NFSU2 | *.BIN *.bin *.VIV *.viv *.BUN *.bun",
            Self::Images => "--file-filter=PNG | *.png *.PNG",
        }
    }
}

/// Bu makinede bir seçici var mı. Çağıran, olmayan bir şeyi açan bir düğme çizmesin diye sorar.
#[must_use]
pub fn available() -> bool {
    chooser().is_some()
}

/// `PATH` üzerinde bulunan ilk seçici.
fn chooser() -> Option<&'static str> {
    CHOOSERS.iter().copied().find(|name| {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {name}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Seçiciyi kendi ipliğinde aç ve sonucu bir kanaldan bildir.
///
/// Kendi ipliğinde, çünkü seçici kullanıcı bir şey seçene kadar dönmüyor: arayüz ipliğinde
/// çalıştırmak pencereyi donduracaktı. İş ipliğinde de değil — orası dosya ayrıştırıyor ve sırayla
/// çalışıyor; açık duran bir pencere yüzünden bir çözme işinin beklemesi için sebep yok.
///
/// `None` gelmesi "vazgeçildi" demek. Kanal kapanırsa iplik zaten bitmiştir.
pub fn open(
    ctx: &egui::Context,
    start_in: Option<PathBuf>,
    filter: Filter,
) -> Option<Receiver<Option<PathBuf>>> {
    let name = chooser()?;
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("pryhub-picker".into())
        .spawn(move || {
            let picked = run(name, start_in.as_deref(), filter);
            let _ = tx.send(picked);
            // Seçici kapandığında pencerenin kendiliğinden uyanması gerekiyor; kimse fare
            // oynatmıyor olabilir.
            ctx.request_repaint();
        })
        .ok()?;
    Some(rx)
}

/// Seçiciyi çalıştır ve seçilen yolu döndür.
fn run(name: &str, start_in: Option<&Path>, filter: Filter) -> Option<PathBuf> {
    let mut cmd = std::process::Command::new(name);
    match name {
        "kdialog" => {
            cmd.arg("--getopenfilename");
            cmd.arg(start_in.map_or_else(|| PathBuf::from("."), Path::to_path_buf));
            cmd.arg(filter.kdialog());
        }
        // zenity, qarma ve yad aynı GTK arayüzünü paylaşıyor.
        _ => {
            cmd.arg("--file-selection");
            cmd.arg("--title=PryHUB");
            if let Some(dir) = start_in {
                // Sondaki ayırıcı zenity'ye bunun bir *klasör* olduğunu söylüyor; onsuz son parçayı
                // dosya adı sanıyor.
                cmd.arg(format!("--filename={}/", dir.display()));
            }
            cmd.arg(filter.gtk());
            cmd.arg("--file-filter=All files | *");
        }
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None; // vazgeçildi
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if path.is_empty() {
        return None;
    }
    // yad birden çok seçimi `|` ile ayırıyor; ilki bize yeter.
    let first = path.split('|').next().unwrap_or(&path);
    Some(PathBuf::from(first))
}
