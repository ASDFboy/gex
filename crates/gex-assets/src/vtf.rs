//! Valve Texture Format (.vtf), versions 7.0–7.5: reads the largest mip of the first
//! frame/face as RGBA8. Covers the formats Source games actually ship (DXT1/3/5 and
//! the plain 8-bit layouts).

use crate::image_load::Rgba;

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

pub const DXT1: u32 = 13;
pub const DXT3: u32 = 14;
pub const DXT5: u32 = 15;
pub const RGBA8888: u32 = 0;
pub const BGRA8888: u32 = 12;

/// Bytes for one w×h image in this format, or None for formats we can't read.
pub fn image_size(format: u32, w: u32, h: u32) -> Option<usize> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let blocks = w.div_ceil(4) * h.div_ceil(4);
    Some(match format {
        13 | 20 => blocks * 8,
        14 | 15 => blocks * 16,
        0 | 1 | 11 | 12 | 16 | 23 | 26 => w * h * 4,
        2 | 3 | 9 | 10 => w * h * 3,
        4 | 17 | 18 | 19 | 21 | 6 | 22 => w * h * 2,
        5 | 7 | 8 => w * h,
        24 | 25 => w * h * 8,
        _ => return None,
    })
}

pub struct VtfInfo {
    pub version: (u32, u32),
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub mips: u32,
    pub frames: u32,
}

pub fn info(b: &[u8]) -> Result<VtfInfo, String> {
    if b.len() < 64 || &b[0..4] != b"VTF\0" {
        return Err("not a VTF file".into());
    }
    Ok(VtfInfo {
        version: (u32le(b, 4), u32le(b, 8)),
        width: u16le(b, 16) as u32,
        height: u16le(b, 18) as u32,
        format: u32le(b, 52),
        mips: b[56] as u32,
        frames: u16le(b, 24).max(1) as u32,
    })
}

pub fn decode(b: &[u8]) -> Result<Rgba, String> {
    let inf = info(b)?;
    let header_size = u32le(b, 12) as usize;
    let flags = u32le(b, 20);
    let (w, h, fmt) = (inf.width, inf.height, inf.format);
    let faces: usize = if flags & 0x4000 != 0 { 6 } else { 1 };
    let depth = if inf.version.1 >= 2 && b.len() > 64 { u16le(b, 63).max(1) as usize } else { 1 };
    let mips = inf.mips.max(1);
    let size0 = image_size(fmt, w, h).ok_or_else(|| format!("VTF image format {} isn't supported yet", fmt))?;

    // Where the high-res data starts.
    let hires_start = if inf.version.1 >= 3 {
        let n = u32le(b, 68) as usize;
        (0..n)
            .map(|i| 80 + i * 8)
            .filter(|o| o + 8 <= b.len())
            .find(|o| b[*o] == 0x30 && b[o + 1] == 0 && b[o + 2] == 0)
            .map(|o| u32le(b, o + 4) as usize)
            .ok_or("VTF has no image data")?
    } else {
        let low_fmt = u32le(b, 57);
        let (lw, lh) = (b[61] as u32, b[62] as u32);
        let low = if low_fmt == u32::MAX || lw == 0 { 0 } else { image_size(low_fmt, lw, lh).unwrap_or(0) };
        header_size + low
    };
    // Mips are stored smallest first; the full-size image is in the last block.
    let mut off = hires_start;
    for m in (1..mips).rev() {
        off += image_size(fmt, (w >> m).max(1), (h >> m).max(1)).unwrap() * inf.frames as usize * faces * depth;
    }
    let data = b.get(off..off + size0).ok_or("VTF image data is cut short")?;
    Ok(Rgba { width: w, height: h, pixels: to_rgba(fmt, w as usize, h as usize, data)? })
}

fn to_rgba(fmt: u32, w: usize, h: usize, d: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; w * h * 4];
    let px = |out: &mut Vec<u8>, i: usize, c: [u8; 4]| out[i * 4..i * 4 + 4].copy_from_slice(&c);
    match fmt {
        13 | 20 | 14 | 15 => decode_dxt(fmt, w, h, d, &mut out),
        _ => {
            for i in 0..w * h {
                let c = match fmt {
                    0 => [d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]],
                    1 => [d[i * 4 + 3], d[i * 4 + 2], d[i * 4 + 1], d[i * 4]],
                    11 => [d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3], d[i * 4]],
                    12 => [d[i * 4 + 2], d[i * 4 + 1], d[i * 4], d[i * 4 + 3]],
                    16 => [d[i * 4 + 2], d[i * 4 + 1], d[i * 4], 255],
                    23 | 26 => [d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]],
                    2 | 9 => [d[i * 3], d[i * 3 + 1], d[i * 3 + 2], 255],
                    3 | 10 => [d[i * 3 + 2], d[i * 3 + 1], d[i * 3], 255],
                    4 | 17 => {
                        let v = u16le(d, i * 2);
                        let (a, b, c) = (((v >> 11) & 31) as u8, ((v >> 5) & 63) as u8, (v & 31) as u8);
                        let (a, b, c) = ((a << 3) | (a >> 2), (b << 2) | (b >> 4), (c << 3) | (c >> 2));
                        if fmt == 4 { [a, b, c, 255] } else { [c, b, a, 255] }
                    }
                    5 => [d[i], d[i], d[i], 255],
                    8 => [255, 255, 255, d[i]],
                    6 => [d[i * 2], d[i * 2], d[i * 2], d[i * 2 + 1]],
                    22 => [d[i * 2], d[i * 2 + 1], 255, 255],
                    _ => return Err(format!("VTF image format {} isn't supported yet", fmt)),
                };
                px(&mut out, i, c);
            }
        }
    }
    Ok(out)
}

fn rgb565(v: u16) -> [u8; 3] {
    let (r, g, b) = (((v >> 11) & 31) as u32, ((v >> 5) & 63) as u32, (v & 31) as u32);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

fn decode_dxt(fmt: u32, w: usize, h: usize, d: &[u8], out: &mut [u8]) {
    let block = if fmt == 13 || fmt == 20 { 8 } else { 16 };
    let bw = w.div_ceil(4);
    for by in 0..h.div_ceil(4) {
        for bx in 0..bw {
            let b = &d[(by * bw + bx) * block..][..block];
            let (alpha, color) = if block == 16 { (&b[..8], &b[8..]) } else { (&b[..0], b) };
            let c0 = u16le(color, 0);
            let c1 = u16le(color, 2);
            let (p0, p1) = (rgb565(c0), rgb565(c1));
            let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
            let mut pal = [[0u8; 4]; 4];
            pal[0] = [p0[0], p0[1], p0[2], 255];
            pal[1] = [p1[0], p1[1], p1[2], 255];
            if c0 > c1 || block == 16 {
                pal[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
                pal[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
            } else {
                pal[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
                pal[3] = [0, 0, 0, 0];
            }
            let bits = u32le(color, 4);
            // DXT5 alpha palette.
            let mut apal = [255u8; 8];
            if fmt == 15 {
                let (a0, a1) = (alpha[0] as u32, alpha[1] as u32);
                apal[0] = a0 as u8;
                apal[1] = a1 as u8;
                for k in 2..8u32 {
                    apal[k as usize] = if a0 > a1 {
                        (((8 - k) * a0 + (k - 1) * a1) / 7) as u8
                    } else if k < 6 {
                        (((6 - k) * a0 + (k - 1) * a1) / 5) as u8
                    } else if k == 6 {
                        0
                    } else {
                        255
                    };
                }
            }
            let abits: u64 = if fmt == 15 { (0..6).fold(0u64, |acc, k| acc | (alpha[2 + k] as u64) << (8 * k)) } else { 0 };
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= w || y >= h {
                        continue;
                    }
                    let k = py * 4 + px;
                    let mut c = pal[((bits >> (2 * k)) & 3) as usize];
                    match fmt {
                        14 => {
                            let a = (u16le(alpha, py * 2) >> (4 * px)) & 15;
                            c[3] = (a as u8) * 17;
                        }
                        15 => c[3] = apal[((abits >> (3 * k)) & 7) as usize],
                        _ => {}
                    }
                    out[(y * w + x) * 4..][..4].copy_from_slice(&c);
                }
            }
        }
    }
}

/// Writes a minimal 7.2 VTF with one mip of RGBA8888 or BGRA8888 pixels (tests/tools).
pub fn write_simple(w: u16, h: u16, format: u32, data: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 80];
    b[0..4].copy_from_slice(b"VTF\0");
    b[4..8].copy_from_slice(&7u32.to_le_bytes());
    b[8..12].copy_from_slice(&2u32.to_le_bytes());
    b[12..16].copy_from_slice(&80u32.to_le_bytes());
    b[16..18].copy_from_slice(&w.to_le_bytes());
    b[18..20].copy_from_slice(&h.to_le_bytes());
    b[24..26].copy_from_slice(&1u16.to_le_bytes());
    b[52..56].copy_from_slice(&format.to_le_bytes());
    b[56] = 1;
    b[57..61].copy_from_slice(&u32::MAX.to_le_bytes());
    b[63..65].copy_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(data);
    b
}
