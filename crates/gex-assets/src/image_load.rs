//! Any image path to RGBA8 pixels: PNG, JPEG, GIF (first frame), BMP, TGA, WebP, TIFF, ICO,
//! QOI, DDS, HDR, EXR, PNM and farbfeld via the `image` crate; SVG (drawn at its own size)
//! and Photoshop .psd (the flattened picture); Valve textures (.vtf) and materials (.vmt,
//! through their $basetexture).

use crate::vfs::Vfs;

#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn load_image(vfs: &Vfs, path: &str) -> Result<Rgba, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".vmt") {
        let m = crate::vmt::parse(&vfs.read_text(path)?);
        let scheme = path.split_once("://").map(|s| s.0).unwrap_or("res");
        let tex = m.base_texture().ok_or_else(|| format!("{} has no $basetexture", path))?;
        return load_image(vfs, &format!("{}://{}", scheme, tex));
    }
    let bytes = vfs.read(path)?;
    decode_image(path, &bytes)
}

/// An image file's pixels from its bytes (any format but .vmt, which points at another file).
/// No Vfs, so it can run on another thread.
pub fn decode_image(path: &str, bytes: &[u8]) -> Result<Rgba, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".vtf") {
        return crate::vtf::decode(bytes).map_err(|e| format!("{}: {}", path, e));
    }
    if lower.ends_with(".svg") {
        return svg(bytes).map_err(|e| format!("{}: {}", path, e));
    }
    if lower.ends_with(".psd") {
        return psd(bytes).map_err(|e| format!("{}: {}", path, e));
    }
    // By its ending first (TGA has no signature to recognise it by), then by its contents.
    let by_name = image::ImageFormat::from_path(crate::model::map_base(path)).ok().and_then(|f| image::load_from_memory_with_format(bytes, f).ok());
    let img = match by_name {
        Some(i) => i,
        None => image::load_from_memory(bytes).map_err(|e| format!("{}: {}", path, e))?,
    }
    .to_rgba8();
    Ok(Rgba { width: img.width(), height: img.height(), pixels: img.into_raw() })
}

/// An image's size in pixels, reading as little of the file as it can.
pub fn image_size(vfs: &Vfs, path: &str) -> Result<[u32; 2], String> {
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".vmt") || lower.ends_with(".vtf") || lower.ends_with(".svg") || lower.ends_with(".psd")) {
        // A file on disk: only its first bytes (where the size is written) are read.
        if let Some(p) = vfs.disk_file(path) {
            if let Ok(r) = image::ImageReader::open(&p).and_then(|r| r.with_guessed_format()) {
                if let Ok((w, h)) = r.into_dimensions() {
                    return Ok([w, h]);
                }
            }
        }
        let bytes = vfs.read(path)?;
        let fmt = image::ImageFormat::from_path(crate::model::map_base(path)).ok();
        let mut r = image::ImageReader::new(std::io::Cursor::new(&bytes));
        match fmt {
            Some(f) => r.set_format(f),
            None => r = r.with_guessed_format().map_err(|e| e.to_string())?,
        }
        if let Ok((w, h)) = r.into_dimensions() {
            return Ok([w, h]);
        }
    }
    load_image(vfs, path).map(|i| [i.width, i.height])
}

pub fn is_image(path: &str) -> bool {
    crate::formats::is_image(path)
}

/// An SVG drawn at its own size (at least 1 pixel; at most 4096 across).
fn svg(bytes: &[u8]) -> Result<Rgba, String> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let k = (4096.0 / size.width().max(size.height())).min(1.0);
    let (w, h) = (((size.width() * k).ceil() as u32).max(1), ((size.height() * k).ceil() as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("an empty picture")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    // tiny-skia keeps premultiplied colour; textures want it straight.
    let mut px = pixmap.take();
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(Rgba { width: w, height: h, pixels: px })
}

/// A Photoshop file's flattened picture (the merged image saved after the layers): RGB or
/// greyscale, 8 or 16 bits, raw or RLE.
fn psd(d: &[u8]) -> Result<Rgba, String> {
    let be16 = |o: usize| d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize).ok_or_else(|| "cut short".to_string());
    let be32 = |o: usize| d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(|| "cut short".to_string());
    if !d.starts_with(b"8BPS") {
        return Err("not a Photoshop file".into());
    }
    let big = be16(4)? == 2;
    let (channels, h, w, depth, mode) = (be16(12)?, be32(14)?, be32(18)?, be16(22)?, be16(24)?);
    if !(depth == 8 || depth == 16) || !(mode == 1 || mode == 3) {
        return Err("only 8 or 16-bit RGB and greyscale Photoshop files can be read".into());
    }
    if w == 0 || h == 0 || w > 30000 || h > 30000 {
        return Err("a picture too big or empty".into());
    }
    // Skip the colour mode data, image resources, and layers.
    let mut at = 26;
    at += 4 + be32(at)?;
    at += 4 + be32(at)?;
    at += if big { 8 + d.get(at..at + 8).map(|b| u64::from_be_bytes(b.try_into().unwrap()) as usize).ok_or("cut short")? } else { 4 + be32(at)? };
    let compression = be16(at)?;
    at += 2;
    let bpc = depth / 8;
    let row = w * bpc;
    let planes = channels.min(4);
    let mut plane_data: Vec<Vec<u8>> = Vec::with_capacity(planes);
    match compression {
        0 => {
            for c in 0..planes {
                let s = at + c * row * h;
                plane_data.push(d.get(s..s + row * h).ok_or("cut short")?.to_vec());
            }
        }
        1 => {
            // PackBits: every row's byte count first, then the rows.
            let count_size = if big { 4 } else { 2 };
            let counts_at = at;
            let mut data_at = at + channels * h * count_size;
            for c in 0..channels {
                let mut plane = Vec::with_capacity(row * h);
                for y in 0..h {
                    let o = counts_at + (c * h + y) * count_size;
                    let n = if big { be32(o)? } else { be16(o)? };
                    let src = d.get(data_at..data_at + n).ok_or("cut short")?;
                    data_at += n;
                    let start = plane.len();
                    let mut i = 0;
                    while i < src.len() && plane.len() - start < row {
                        let k = src[i] as i8;
                        i += 1;
                        if k >= 0 {
                            let n = k as usize + 1;
                            plane.extend_from_slice(src.get(i..i + n).ok_or("cut short")?);
                            i += n;
                        } else if k != -128 {
                            let v = *src.get(i).ok_or("cut short")?;
                            i += 1;
                            plane.extend(std::iter::repeat_n(v, 1 - k as isize as usize));
                        }
                    }
                    plane.resize(start + row, 0);
                }
                if c < planes {
                    plane_data.push(plane);
                }
            }
        }
        _ => return Err("this Photoshop file's picture is zip-compressed: save it with Maximize Compatibility".into()),
    }
    let mut px = Vec::with_capacity(w * h * 4);
    let sample = |p: &Vec<u8>, i: usize| p[i * bpc];
    for i in 0..w * h {
        let (r, g, b, a) = if mode == 3 {
            (sample(&plane_data[0], i), sample(&plane_data[1.min(planes - 1)], i), sample(&plane_data[2.min(planes - 1)], i), if planes > 3 { sample(&plane_data[3], i) } else { 255 })
        } else {
            let v = sample(&plane_data[0], i);
            (v, v, v, if planes > 1 { sample(&plane_data[1], i) } else { 255 })
        };
        px.extend_from_slice(&[r, g, b, a]);
    }
    Ok(Rgba { width: w as u32, height: h as u32, pixels: px })
}
