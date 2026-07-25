//! `--shot <png>`: draw the window, save it, exit.
//!
//! This machine's compositor will not hand out a screen grab, so this flag is the only way to look
//! at the interface — every screenshot in the project's commit messages came from it. It waits for
//! two things before pressing the shutter: the worker going quiet, or it photographs a window that
//! has not loaded anything yet, and then a moment of wall-clock time for the animations to land.
//! Frames are drawn back to back here, so counting frames would not do it: an animation measured in
//! milliseconds needs milliseconds, not repaints.
//!
//! The PNG is written by hand rather than through the `png` crate: this is one file's worth of
//! deflate-free encoding, and the app has no other reason to carry an image encoder.

use crate::app::PryHub;

/// A pending `--shot` request.
pub(crate) struct Shot {
    pub(crate) path: std::path::PathBuf,
    /// Frames to draw before asking for the image: the first frame has no layout yet, and the
    /// font atlas is built lazily.
    pub(crate) warmup: u8,
    /// When the animations will have finished. Set once the worker goes quiet.
    pub(crate) settled_at: Option<std::time::Instant>,
    pub(crate) asked: bool,
}

/// Long enough for every animation in the interface to arrive (the longest is
/// [`crate::chrome::MOVE_TIME`]), short enough not to slow a scripted screenshot noticeably.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

impl PryHub {
    /// Drive a pending `--shot`: warm up, ask for the image, save it, quit.
    pub(crate) fn screenshot(&mut self, ctx: &egui::Context) {
        // Opening and decoding are jobs now, so a screenshot taken on frame four would catch an
        // empty window. Wait for the worker to go quiet first — the flag exists to check the
        // interface, and an interface that has not loaded anything yet is not the one to check.
        let busy = self.jobs.busy().is_some();
        let Some(shot) = &mut self.shot else { return };
        if busy {
            ctx.request_repaint();
            return;
        }
        if shot.warmup > 0 {
            shot.warmup -= 1;
            ctx.request_repaint();
            return;
        }
        // Let the sliding underline and the fading thumbnails finish, so two runs of the same
        // command produce the same picture.
        let deadline = *shot.settled_at.get_or_insert_with(|| std::time::Instant::now() + SETTLE);
        if std::time::Instant::now() < deadline {
            ctx.request_repaint();
            return;
        }
        if !shot.asked {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.asked = true;
            ctx.request_repaint();
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let (w, h) = (image.width() as u32, image.height() as u32);
            match write_png(&shot.path, &rgba, w, h) {
                Ok(()) => eprintln!("pryhub: {} ({w}x{h})", shot.path.display()),
                Err(e) => eprintln!("pryhub: {}: {e}", shot.path.display()),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Write RGBA8 as a PNG. Hand-rolled (stored deflate + CRC) so a debugging convenience does not
/// add a dependency to the tool everyone builds.
fn write_png(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    use std::io::Write as _;
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut with_tag = tag.to_vec();
        with_tag.extend_from_slice(body);
        out.extend_from_slice(&with_tag);
        out.extend_from_slice(&crc32(&with_tag).to_be_bytes());
    }
    // Raw scanlines: filter byte 0 per row.
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        let row = y * w as usize * 4;
        raw.extend_from_slice(&rgba[row..row + w as usize * 4]);
    }
    // zlib stream with stored (uncompressed) deflate blocks — valid, just not small.
    let mut z = vec![0x78, 0x01];
    let mut adler = (1u32, 0u32);
    for &b in &raw {
        adler.0 = (adler.0 + u32::from(b)) % 65521;
        adler.1 = (adler.1 + adler.0) % 65521;
    }
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = u8::from((i + 1) * 65535 >= raw.len());
        z.push(last);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&((adler.1 << 16) | adler.0).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&png)
}

