//! Turning parsed data back into files other tools read: OBJ + MTL, and PNG.
//!
//! Everything here is a pure function of already-parsed data — it returns text or bytes and never
//! touches the filesystem, so the caller decides where a file goes (and a GUI may put it on a
//! clipboard instead). What belongs here is the *format knowledge* an export needs: which material
//! a run resolves to, which frame the vertices are in, what a texture is called when the file
//! gives it no name. Keeping that in the library is what stops `ug2` and PryHUB from drifting into
//! two different answers for the same car.

/// Binary glTF. Behind the `png` feature because a `.glb` embeds its images, and an exporter that
/// silently dropped them would produce an untextured car rather than a smaller file.
#[cfg(feature = "png")]
pub mod gltf;
pub mod material;
pub mod obj;

#[cfg(feature = "png")]
pub use gltf::write_glb;
pub use material::MaterialPlan;
pub use obj::{write_mtl, write_obj, Material};

use crate::types::NfsTexture;

/// A texture's file name: its `DebugName` when the compiler left a usable one, else its hash.
///
/// The hash is appended either way — names in a TPK are truncated to a fixed field, so two
/// different textures can arrive under one name and only the hash tells them apart.
#[must_use]
pub fn png_name(t: &NfsTexture) -> String {
    let stem: String = t.name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if stem.is_empty() {
        format!("{:08X}.png", t.hash.0)
    } else {
        format!("{stem}_{:08X}.png", t.hash.0)
    }
}

/// Encode a decoded texture as PNG bytes (RGBA8, no filtering choices of our own).
///
/// # Errors
/// [`NfsError::BufferSizeMismatch`](crate::NfsError::BufferSizeMismatch) when the pixel buffer
/// does not match the descriptor's width × height, and [`NfsError::Io`](crate::NfsError::Io) if
/// the encoder itself fails.
#[cfg(feature = "png")]
pub fn png_bytes(t: &NfsTexture) -> crate::NfsResult<Vec<u8>> {
    let expected = t.width as usize * t.height as usize * 4;
    if t.rgba.len() != expected {
        return Err(crate::NfsError::BufferSizeMismatch {
            detail: "texture pixel buffer does not match its width × height",
        });
    }
    let mut out = Vec::new();
    let io = |e: png::EncodingError| crate::NfsError::Io(std::io::Error::other(e.to_string()));
    let mut enc = png::Encoder::new(&mut out, t.width, t.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(io)?;
    writer.write_image_data(&t.rgba).map_err(io)?;
    writer.finish().map_err(io)?;
    Ok(out)
}

/// Read a PNG back into RGBA8 pixels and its dimensions — the other direction of [`png_bytes`].
///
/// It lives here, in the module about turning parsed data into files other tools read, because this
/// is where the crate's PNG dependency already is. A texture written out as a PNG, edited in
/// anything, and handed back to [`crate::texture::replace_pixels`] makes a round trip, and both ends
/// of it should be the same code — `ug2` and PryHUB reading a user's image through two different
/// decoders is exactly the kind of drift the export writers are shared to avoid.
///
/// Whatever the file says it is, what comes back is RGBA8: palette and greyscale images are expanded,
/// 16-bit channels are reduced to 8, and an image with no alpha gets an opaque one. That is not
/// leniency for its own sake — a person exporting a texture, opening it in an editor and saving it
/// again very often gets back RGB rather than RGBA, and refusing that would be refusing the
/// commonest thing that happens to a file between coming out of here and going back in.
///
/// # Errors
/// [`NfsError::Io`](crate::NfsError::Io) when the bytes are not a PNG this decoder reads, and
/// [`NfsError::BufferSizeMismatch`](crate::NfsError::BufferSizeMismatch) when the frame it hands
/// back is not the size its own header declared.
/// A PNG's dimensions, from its header alone.
///
/// The cheap half of [`png_pixels`], and it exists because the expensive half is the wrong thing to
/// do on the frame a user is looking at. A replacement has to have the texture's own dimensions, so
/// that is the question an interface asks the moment a file is chosen — and answering it by decoding
/// a 512² image costs the pixels, the allocation and the wait, to compare two integers that are in
/// the first twenty-four bytes of the file.
///
/// # Errors
/// [`NfsError::Io`](crate::NfsError::Io) when the bytes are not a PNG this decoder reads.
#[cfg(feature = "png")]
pub fn png_size(bytes: &[u8]) -> crate::NfsResult<(u32, u32)> {
    let io = |e: png::DecodingError| crate::NfsError::Io(std::io::Error::other(e.to_string()));
    let reader = png::Decoder::new(bytes).read_info().map_err(io)?;
    Ok(reader.info().size())
}

#[cfg(feature = "png")]
pub fn png_pixels(bytes: &[u8]) -> crate::NfsResult<(Vec<u8>, u32, u32)> {
    let io = |e: png::DecodingError| crate::NfsError::Io(std::io::Error::other(e.to_string()));
    let mut decoder = png::Decoder::new(bytes);
    // EXPAND turns palette and low-bit-depth greyscale into full channels; STRIP_16 brings 16-bit
    // channels down to the 8 this crate's textures are. Together they leave four possible colour
    // types, and the match below is exhaustive over them.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(io)?;
    // The output buffer's size comes from the header's width × height, and allocating from an
    // unchecked size field is the one thing this crate does not do — the `png` crate's own 64 MiB
    // limit covers its internal buffers and not this one, so a four-byte IHDR claiming 65535 square
    // would ask for seventeen gigabytes here before anything looked at the pixels.
    //
    // The cap is twice [`crate::texture`]'s own `MAX_DIM`, so it can never be the thing that refuses
    // a legitimate replacement: the largest texture in the game is 1024².
    const MAX_SIDE: u32 = 8192;
    let (w, h) = reader.info().size();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(crate::NfsError::BufferSizeMismatch {
            detail: "PNG dimensions out of range",
        });
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(io)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = buf.get(..info.buffer_size()).ok_or(crate::NfsError::BufferSizeMismatch {
        detail: "PNG frame shorter than its own header declares",
    })?;

    let mut rgba = vec![0u8; w * h * 4];
    let widen = |dst: &mut [u8], src: &[u8], stride: usize, to: fn(&[u8]) -> [u8; 4]| {
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(stride)) {
            d.copy_from_slice(&to(s));
        }
    };
    match info.color_type {
        png::ColorType::Rgba => widen(&mut rgba, src, 4, |s| [s[0], s[1], s[2], s[3]]),
        png::ColorType::Rgb => widen(&mut rgba, src, 3, |s| [s[0], s[1], s[2], 255]),
        png::ColorType::GrayscaleAlpha => widen(&mut rgba, src, 2, |s| [s[0], s[0], s[0], s[1]]),
        png::ColorType::Grayscale => widen(&mut rgba, src, 1, |s| [s[0], s[0], s[0], 255]),
        // EXPAND has already turned an indexed image into one of the above, so this is unreachable
        // in practice — and is an error rather than an `unreachable!`, because "in practice" is not
        // a thing this crate says about input it did not write.
        png::ColorType::Indexed => {
            return Err(crate::NfsError::NotImplemented { feature: "indexed PNG" })
        }
    }
    Ok((rgba, info.width, info.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssetHash;

    fn tex(name: &str, w: u32, h: u32) -> NfsTexture {
        NfsTexture {
            name: name.to_string(),
            hash: AssetHash(0x1234_ABCD),
            width: w,
            height: h,
            rgba: vec![0x7F; (w * h * 4) as usize],
            ..NfsTexture::default()
        }
    }

    #[test]
    fn a_nameless_texture_is_named_after_its_hash() {
        assert_eq!(png_name(&tex("", 2, 2)), "1234ABCD.png");
        // The hash stays on the end even when there is a name: TPK names are truncated, so two
        // textures can share one.
        assert_eq!(png_name(&tex("240SX_BADGING", 2, 2)), "240SX_BADGING_1234ABCD.png");
    }

    #[test]
    fn a_name_with_punctuation_keeps_only_what_a_filename_may_hold() {
        assert_eq!(png_name(&tex("front/left.b", 2, 2)), "frontleftb_1234ABCD.png");
    }

    #[cfg(feature = "png")]
    #[test]
    fn png_bytes_start_with_the_png_signature() {
        let bytes = png_bytes(&tex("t", 4, 2)).expect("4×2 RGBA must encode");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[cfg(feature = "png")]
    #[test]
    fn a_pixel_buffer_that_does_not_match_the_descriptor_is_refused() {
        let mut t = tex("t", 4, 2);
        t.rgba.truncate(7);
        assert!(png_bytes(&t).is_err());
    }

    /// The round trip this crate now owns both ends of: out as a PNG, back as pixels.
    #[cfg(feature = "png")]
    #[test]
    fn a_png_written_here_reads_back_here() {
        let mut t = tex("t", 4, 2);
        for (i, px) in t.rgba.chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[i as u8 * 30, 255 - i as u8 * 20, 7, 200 - i as u8]);
        }
        let bytes = png_bytes(&t).expect("encode");
        let (rgba, w, h) = png_pixels(&bytes).expect("decode");
        assert_eq!((w, h), (4, 2));
        assert_eq!(rgba, t.rgba);
    }

    /// The commonest thing an image editor does to a texture on its way back: drop the alpha.
    #[cfg(feature = "png")]
    #[test]
    fn a_png_without_alpha_comes_back_opaque() {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("header");
        w.write_image_data(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).expect("data");
        w.finish().expect("finish");

        let (rgba, width, height) = png_pixels(&out).expect("decode");
        assert_eq!((width, height), (2, 2));
        assert_eq!(rgba, vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
    }

    /// And a greyscale one, which an editor produces from a mask as readily as an RGB one.
    #[cfg(feature = "png")]
    #[test]
    fn a_greyscale_png_comes_back_as_three_equal_channels() {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 2, 1);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("header");
        w.write_image_data(&[40, 200]).expect("data");
        w.finish().expect("finish");

        let (rgba, ..) = png_pixels(&out).expect("decode");
        assert_eq!(rgba, vec![40, 40, 40, 255, 200, 200, 200, 255]);
    }

    #[cfg(feature = "png")]
    #[test]
    fn bytes_that_are_not_a_png_are_an_error_rather_than_a_panic() {
        assert!(png_pixels(b"not a png at all").is_err());
        assert!(png_pixels(&[]).is_err());
    }

    /// A header claiming an enormous image is refused before the buffer it asks for is allocated.
    ///
    /// Built by hand rather than by the encoder, because the encoder will not write one: the point
    /// is a file whose IHDR promises more than its data delivers, which is what a hostile or a
    /// truncated PNG looks like.
    #[cfg(feature = "png")]
    #[test]
    fn a_png_claiming_an_enormous_image_is_refused_before_it_is_allocated() {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = crc32(kind);
            crc = crc32_continue(crc, data);
            out.extend_from_slice(&crc.to_be_bytes());
            out
        }
        // 60000 × 60000 RGBA would be 14 GB of output buffer.
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive, no interlace
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&chunk(b"IHDR", &ihdr));
        png.extend_from_slice(&chunk(b"IDAT", &[]));
        png.extend_from_slice(&chunk(b"IEND", &[]));
        assert!(png_pixels(&png).is_err());
    }

    /// CRC-32 of a PNG chunk's type and data, so the fixture above is a file a decoder will read
    /// far enough to reject for the right reason rather than for a bad checksum.
    #[cfg(feature = "png")]
    fn crc32(data: &[u8]) -> u32 {
        // Zero, because `crc32_continue` un-finalises what it is given and a fresh run starts from
        // the all-ones register: `0 ^ 0xFFFF_FFFF` is that register.
        crc32_continue(0, data)
    }

    #[cfg(feature = "png")]
    fn crc32_continue(crc: u32, data: &[u8]) -> u32 {
        let mut c = crc ^ 0xFFFF_FFFF;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        c ^ 0xFFFF_FFFF
    }
}
