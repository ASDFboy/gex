//! Label2D text drawn as pictures, so text sits in layer order with sprites (a label can be
//! under a sprite, or between two). Each label's text is turned into a picture once (and
//! again only when its text, size, font or tag colors change).
//!
//! The same tags as the editor's game text: `[color=#ff8800]..[/color]` (or a name),
//! `[size=24]..[/size]`, `[i]..[/i]`, `[u]..[/u]`, `[s]..[/s]`, `[bg=#000000]..[/bg]`.
//! Anything else in square brackets is shown as written.

use std::collections::HashMap;

use ab_glyph::{Font, FontArc, Glyph, PxScale, ScaleFont};

/// One run of text with one look.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    /// None: the label's own color.
    pub color: Option<[u8; 4]>,
    pub bg: Option<[u8; 4]>,
    /// Size in game pixels ([size=..]); None: the label's size.
    pub size: Option<f32>,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let t = s.trim().to_ascii_lowercase();
    let named = match t.as_str() {
        "white" => Some((255, 255, 255)),
        "black" => Some((0, 0, 0)),
        "red" => Some((230, 51, 51)),
        "green" => Some((77, 204, 77)),
        "blue" => Some((64, 115, 242)),
        "yellow" => Some((242, 217, 51)),
        "orange" => Some((242, 140, 38)),
        "purple" => Some((153, 77, 217)),
        "gray" | "grey" => Some((128, 128, 128)),
        _ => None,
    };
    if let Some((r, g, b)) = named {
        return Some([r, g, b, 255]);
    }
    let h = t.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    // #rgb and #rgba: each digit twice (#f80 is #ff8800).
    let q = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
    match h.len() {
        3 => Some([q(0)?, q(1)?, q(2)?, 255]),
        4 => Some([q(0)?, q(1)?, q(2)?, q(3)?]),
        6 => Some([p(0)?, p(2)?, p(4)?, 255]),
        8 => Some([p(0)?, p(2)?, p(4)?, p(6)?]),
        _ => None,
    }
}

/// Text with tags → runs of text.
pub fn parse(text: &str) -> Vec<Span> {
    let plain = Span { text: String::new(), color: None, bg: None, size: None, italic: false, underline: false, strike: false };
    let mut out: Vec<Span> = Vec::new();
    let mut stack: Vec<(String, Span)> = Vec::new();
    let mut cur = plain;
    let mut rest = text;
    let add = |s: &Span, t: &str, out: &mut Vec<Span>| {
        if t.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(l) if Span { text: String::new(), ..l.clone() } == Span { text: String::new(), ..s.clone() } => l.text.push_str(t),
            _ => out.push(Span { text: t.to_string(), ..s.clone() }),
        }
    };
    while !rest.is_empty() {
        let Some(open) = rest.find('[') else {
            add(&cur, rest, &mut out);
            break;
        };
        let Some(close) = rest[open..].find(']').map(|c| open + c) else {
            add(&cur, rest, &mut out);
            break;
        };
        let tag = &rest[open + 1..close];
        let (name, arg) = tag.split_once('=').map(|(a, b)| (a.trim(), Some(b.trim()))).unwrap_or((tag.trim(), None));
        let lname = name.to_ascii_lowercase();
        let mut handled = true;
        let mut next = cur.clone();
        if let Some(end) = lname.strip_prefix('/') {
            match stack.iter().rposition(|(t, _)| t == end) {
                Some(i) => {
                    next = stack[i].1.clone();
                    stack.truncate(i);
                }
                None => handled = false,
            }
        } else {
            match (lname.as_str(), arg) {
                ("color", Some(c)) => match parse_color(c) {
                    Some(c) => next.color = Some(c),
                    None => handled = false,
                },
                ("bg", Some(c)) => match parse_color(c) {
                    Some(c) => next.bg = Some(c),
                    None => handled = false,
                },
                ("size", Some(n)) => match n.parse::<f32>() {
                    Ok(n) => next.size = Some(n.max(1.0)),
                    Err(_) => handled = false,
                },
                ("i", None) => next.italic = true,
                ("u", None) => next.underline = true,
                ("s", None) => next.strike = true,
                _ => handled = false,
            }
            if handled {
                stack.push((lname.clone(), cur.clone()));
            }
        }
        if handled {
            add(&cur, &rest[..open], &mut out);
            cur = next;
        } else {
            add(&cur, &rest[..=close], &mut out);
        }
        rest = &rest[close + 1..];
    }
    out
}

/// Fonts: egui's default ones (always there), and the project's font files as they're asked for.
pub struct Fonts {
    main: FontArc,
    fallbacks: Vec<FontArc>,
    files: HashMap<String, Option<FontArc>>,
}

impl Default for Fonts {
    fn default() -> Self {
        let f = |b: &'static [u8]| FontArc::try_from_slice(b).expect("built-in font");
        Fonts {
            main: f(epaint_default_fonts::UBUNTU_LIGHT),
            fallbacks: vec![f(epaint_default_fonts::HACK_REGULAR), f(epaint_default_fonts::NOTO_EMOJI_REGULAR), f(epaint_default_fonts::EMOJI_ICON)],
            files: HashMap::new(),
        }
    }
}

impl Fonts {
    /// The font for a file path ("" = the default), loading it the first time.
    pub fn get(&mut self, vfs: &crate::Vfs, path: &str, problems: &mut Vec<String>) -> FontArc {
        if path.is_empty() {
            return self.main.clone();
        }
        if !self.files.contains_key(path) {
            // A name without a scheme ("Times New Roman") is a font installed on the computer.
            let bytes = if path.contains("://") {
                vfs.read(path)
            } else {
                system_font(path).and_then(|p| std::fs::read(&p).ok()).ok_or_else(|| format!("no font called {} is installed on this computer", path))
            };
            let f = match bytes {
                Ok(b) => match FontArc::try_from_vec(b) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        problems.push(format!("font {}: {}", path, e));
                        None
                    }
                },
                Err(e) => {
                    problems.push(format!("font {}: {}", path, e));
                    None
                }
            };
            self.files.insert(path.to_string(), f);
        }
        self.files[path].clone().unwrap_or_else(|| self.main.clone())
    }

    /// The font that has a character (and a number telling which it is): the chosen one,
    /// else the default, else the first fallback that does.
    fn for_char(&self, first: &FontArc, c: char) -> (usize, FontArc) {
        if first.glyph_id(c).0 != 0 || c.is_whitespace() {
            return (0, first.clone());
        }
        if self.main.glyph_id(c).0 != 0 {
            return (1, self.main.clone());
        }
        match self.fallbacks.iter().position(|f| f.glyph_id(c).0 != 0) {
            Some(i) => (2 + i, self.fallbacks[i].clone()),
            None => (0, first.clone()),
        }
    }
}

/// Text as a picture.
pub struct TextImage {
    pub w: u32,
    pub h: u32,
    /// Straight (not premultiplied) RGBA, sRGB colors.
    pub rgba: Vec<u8>,
    /// The picture's pixels per game pixel.
    pub scale: f32,
    /// Empty space around the text, in the picture's pixels (for the outline).
    pub pad: f32,
}

/// How to draw a piece of text.
pub struct Style<'a> {
    pub font: &'a str,
    /// Size in game pixels.
    pub size: f32,
    /// Picture pixels per game pixel (zoom × resolution): text stays sharp when zoomed.
    pub scale: f32,
    /// The color where no tag sets one.
    pub color: [u8; 4],
    /// Lines line up: 0 left, 0.5 centered, 1 right.
    pub align: f32,
    /// Wrap lines longer than this (game pixels); 0 = no wrapping.
    pub wrap: f32,
    /// Outline width (game pixels) and color.
    pub outline: f32,
    pub outline_color: [u8; 4],
}

struct Placed {
    font: FontArc,
    glyph: Glyph,
    color: [u8; 4],
    italic: bool,
    baseline: f32,
}

/// A rectangle to fill: x0, y0, x1, y1, color.
type Fill = (f32, f32, f32, f32, [u8; 4]);

/// Splits runs into lines: at line breaks, and (with `wrap`) at the last space that keeps a
/// line within `wrap` picture pixels.
fn lines_of(fonts: &Fonts, base: &FontArc, spans: &[Span], base_px: f32, scale: f32, wrap: f32) -> Vec<Vec<(Span, String)>> {
    let mut rows: Vec<Vec<(Span, String)>> = vec![Vec::new()];
    let mut x = 0.0f32;
    let width = |s: &Span, t: &str| -> f32 {
        let px = s.size.map_or(base_px, |n| (n * scale).max(1.0));
        t.chars().map(|c| fonts.for_char(base, c).1.as_scaled(PxScale::from(px)).h_advance(fonts.for_char(base, c).1.glyph_id(c))).sum()
    };
    for s in spans {
        for (i, part) in s.text.split('\n').enumerate() {
            if i > 0 {
                rows.push(Vec::new());
                x = 0.0;
            }
            if wrap <= 0.0 {
                if !part.is_empty() {
                    rows.last_mut().unwrap().push((s.clone(), part.to_string()));
                }
                continue;
            }
            // Words, each with the spaces after it.
            let mut words: Vec<&str> = Vec::new();
            let mut st = 0;
            for (j, c) in part.char_indices() {
                if c == ' ' && part[j + 1..].chars().next().is_some_and(|n| n != ' ') {
                    words.push(&part[st..j + 1]);
                    st = j + 1;
                }
            }
            if st < part.len() {
                words.push(&part[st..]);
            }
            for w in words {
                let ww = width(s, w.trim_end());
                if x > 0.0 && x + ww > wrap {
                    rows.push(Vec::new());
                    x = 0.0;
                }
                let row = rows.last_mut().unwrap();
                match row.last_mut() {
                    Some((ls, lt)) if *ls == *s => lt.push_str(w),
                    _ => row.push((s.clone(), w.to_string())),
                }
                x += width(s, w);
            }
        }
    }
    // Spaces at the end of a wrapped line don't count.
    if wrap > 0.0 {
        for r in &mut rows {
            if let Some((_, t)) = r.last_mut() {
                let k = t.trim_end_matches(' ').len();
                t.truncate(k);
            }
        }
    }
    rows
}

/// Draws text into a picture.
pub fn rasterize(fonts: &mut Fonts, vfs: &crate::Vfs, text: &str, style: &Style, problems: &mut Vec<String>) -> TextImage {
    build(fonts, vfs, text, style, problems, true)
}

/// How big a piece of text is, in game pixels (its picture without the room for an outline):
/// laid out exactly as it's drawn, without drawing it.
pub fn measure(fonts: &mut Fonts, vfs: &crate::Vfs, text: &str, style: &Style, problems: &mut Vec<String>) -> [f32; 2] {
    let i = build(fonts, vfs, text, style, problems, false);
    let s = i.scale.max(1e-6);
    [((i.w as f32 - i.pad * 2.0) / s).max(0.0), ((i.h as f32 - i.pad * 2.0) / s).max(0.0)]
}

fn build(fonts: &mut Fonts, vfs: &crate::Vfs, text: &str, style: &Style, problems: &mut Vec<String>, pixels: bool) -> TextImage {
    let base = fonts.get(vfs, style.font, problems);
    let scale = style.scale;
    let spans = parse(text);
    let outline = (style.outline * scale).max(0.0);
    let pad = 2.0 + outline.ceil();
    let base_px = (style.size * scale).max(1.0);
    let rows = lines_of(fonts, &base, &spans, base_px, scale, style.wrap * scale);
    let mut placed: Vec<Placed> = Vec::new();
    let mut bgs: Vec<Fill> = Vec::new();
    let mut lines: Vec<Fill> = Vec::new();
    // Each line's width and where its glyphs and rectangles start (to line them up after).
    let mut row_info: Vec<(f32, usize, usize, usize)> = Vec::new();
    let mut y = pad;
    let mut width = 0.0f32;
    for row in &rows {
        let (p0, b0, l0) = (placed.len(), bgs.len(), lines.len());
        let m = base.as_scaled(PxScale::from(base_px));
        let (mut asc, mut desc, mut gap) = (m.ascent(), m.descent(), m.line_gap());
        for (s, _) in row {
            let px = s.size.map_or(base_px, |n| (n * scale).max(1.0));
            let sf = base.as_scaled(PxScale::from(px));
            asc = asc.max(sf.ascent());
            desc = desc.min(sf.descent());
            gap = gap.max(sf.line_gap());
        }
        let baseline = y + asc;
        let mut x = pad;
        for (s, part) in row {
            let px = s.size.map_or(base_px, |n| (n * scale).max(1.0));
            let col = s.color.unwrap_or(style.color);
            let x0 = x;
            let mut last: Option<(ab_glyph::GlyphId, usize)> = None;
            for c in part.chars() {
                let (key, f) = fonts.for_char(&base, c);
                let sf = f.as_scaled(PxScale::from(px));
                let id = sf.glyph_id(c);
                if let Some((prev, k)) = last {
                    if k == key {
                        x += sf.kern(prev, id);
                    }
                }
                let g = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(x, baseline));
                x += sf.h_advance(id);
                last = Some((id, key));
                placed.push(Placed { font: f, glyph: g, color: col, italic: s.italic, baseline });
            }
            let thick = (px / 14.0).max(1.0);
            let sf = base.as_scaled(PxScale::from(px));
            if let Some(b) = s.bg {
                bgs.push((x0, baseline - sf.ascent(), x, baseline - sf.descent(), b));
            }
            if s.underline {
                lines.push((x0, baseline + thick * 1.5, x, baseline + thick * 2.5, col));
            }
            if s.strike {
                let m = baseline - sf.ascent() * 0.32;
                lines.push((x0, m - thick / 2.0, x, m + thick / 2.0, col));
            }
            if s.italic {
                x += px * 0.08;
            }
        }
        row_info.push((x - pad, p0, b0, l0));
        width = width.max(x - pad);
        y = baseline - desc + gap;
    }
    // Line each line up within the widest.
    if style.align > 0.0 {
        let ends: Vec<(usize, usize, usize)> = row_info.iter().skip(1).map(|r| (r.1, r.2, r.3)).chain(std::iter::once((placed.len(), bgs.len(), lines.len()))).collect();
        for (r, end) in row_info.iter().zip(ends) {
            let dx = (width - r.0) * style.align;
            if dx == 0.0 {
                continue;
            }
            for p in &mut placed[r.1..end.0] {
                p.glyph.position.x += dx;
            }
            for f in bgs[r.2..end.1].iter_mut().chain(lines[r.3..end.2].iter_mut()) {
                f.0 += dx;
                f.2 += dx;
            }
        }
    }
    let w = (width + pad * 2.0).ceil().max(1.0) as u32;
    let h = (y + pad).ceil().max(1.0) as u32;
    if !pixels {
        return TextImage { w, h, rgba: Vec::new(), scale, pad };
    }
    let mut img = vec![0u8; (w * h * 4) as usize];
    for r in bgs {
        fill(&mut img, w, h, r);
    }
    for p in placed {
        let Some(o) = p.font.outline_glyph(p.glyph) else { continue };
        let bb = o.px_bounds();
        o.draw(|gx, gy, cov| {
            let py = bb.min.y + gy as f32;
            // Slanted for [i]: higher pixels lean right.
            let lean = if p.italic { (p.baseline - py) * 0.2 } else { 0.0 };
            over(&mut img, w, h, (bb.min.x + gx as f32 + lean).round() as i32, py as i32, p.color, cov);
        });
    }
    for r in lines {
        fill(&mut img, w, h, r);
    }
    if outline > 0.0 {
        img = outlined(&img, w, h, outline, style.outline_color);
    }
    TextImage { w, h, rgba: img, scale, pad }
}

/// Paints a color over a pixel, `cov` of the way (straight alpha).
fn over(img: &mut [u8], w: u32, h: u32, px: i32, py: i32, c: [u8; 4], cov: f32) {
    if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 || cov <= 0.0 {
        return;
    }
    let i = ((py as u32 * w + px as u32) * 4) as usize;
    let sa = c[3] as f32 / 255.0 * cov.min(1.0);
    let da = img[i + 3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return;
    }
    for k in 0..3 {
        let v = (c[k] as f32 * sa + img[i + k] as f32 * da * (1.0 - sa)) / oa;
        img[i + k] = v.round().clamp(0.0, 255.0) as u8;
    }
    img[i + 3] = (oa * 255.0).round() as u8;
}

fn fill(img: &mut [u8], w: u32, h: u32, r: Fill) {
    for py in r.1.floor() as i32..r.3.ceil() as i32 {
        for px in r.0.floor() as i32..r.2.ceil() as i32 {
            over(img, w, h, px, py, r.4, 1.0);
        }
    }
}

/// The text with an outline of `r` pixels behind it.
fn outlined(img: &[u8], w: u32, h: u32, r: f32, color: [u8; 4]) -> Vec<u8> {
    let ri = r.ceil() as i32;
    let (wi, hi) = (w as i32, h as i32);
    let alpha = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= wi || y >= hi {
            0.0
        } else {
            img[((y * wi + x) * 4 + 3) as usize] as f32 / 255.0
        }
    };
    // The offsets within the circle (with a soft edge), once.
    let mut ring: Vec<(i32, i32, f32)> = Vec::new();
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let k = (r + 0.5 - d).clamp(0.0, 1.0);
            if k > 0.0 {
                ring.push((dx, dy, k));
            }
        }
    }
    let mut out = vec![0u8; img.len()];
    for y in 0..hi {
        for x in 0..wi {
            let mut a = 0.0f32;
            for &(dx, dy, k) in &ring {
                a = a.max(alpha(x + dx, y + dy) * k);
                if a >= 1.0 {
                    break;
                }
            }
            if a > 0.0 {
                over(&mut out, w, h, x, y, color, a);
            }
            let i = ((y * wi + x) * 4) as usize;
            let c = [img[i], img[i + 1], img[i + 2], 255];
            let cov = img[i + 3] as f32 / 255.0;
            over(&mut out, w, h, x, y, c, cov);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        let s = parse("a [color=red]b [i]c[/i][/color] [nope] d");
        let t: String = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(t, "a b c [nope] d");
        assert_eq!(s[0].color, None);
        assert_eq!(s[1].color, Some([230, 51, 51, 255]));
        assert!(s[2].italic && s[2].color.is_some());
        assert!(!s.last().unwrap().italic && s.last().unwrap().color.is_none());
    }

    #[test]
    fn draws_text() {
        let vfs = crate::Vfs::new(std::path::Path::new("."), &Default::default());
        let mut fonts = Fonts::default();
        let mut problems = Vec::new();
        let st = |scale: f32, wrap: f32, outline: f32| Style { font: "", size: 16.0, scale, color: [255; 4], align: 0.5, wrap, outline, outline_color: [0, 0, 0, 255] };
        let a = rasterize(&mut fonts, &vfs, "Hi", &st(1.0, 0.0, 0.0), &mut problems);
        let b = rasterize(&mut fonts, &vfs, "Hi there\nsecond line", &st(2.0, 0.0, 0.0), &mut problems);
        assert!(a.w > 10 && a.h >= 16 && a.rgba.chunks(4).any(|p| p[3] > 200));
        assert!(b.w > a.w * 3 && b.h > a.h * 3);
        // Wrapping makes it taller and narrower.
        let c = rasterize(&mut fonts, &vfs, "one two three four five six", &st(1.0, 60.0, 0.0), &mut problems);
        let d = rasterize(&mut fonts, &vfs, "one two three four five six", &st(1.0, 0.0, 0.0), &mut problems);
        assert!(c.h > d.h * 2 && c.w < d.w / 2, "{}x{} vs {}x{}", c.w, c.h, d.w, d.h);
        // An outline adds dark pixels around the white ones.
        let e = rasterize(&mut fonts, &vfs, "Hi", &st(1.0, 0.0, 2.0), &mut problems);
        assert!(e.rgba.chunks(4).any(|p| p[3] > 200 && p[0] < 30) && e.rgba.chunks(4).any(|p| p[0] > 220 && p[3] > 200));
        assert!(problems.is_empty());
    }
}

/// Where fonts are installed on this computer.
fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut d = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from);
    if cfg!(windows) {
        let win = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        d.push(win.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            d.push(std::path::PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        d.push("/System/Library/Fonts".into());
        d.push("/Library/Fonts".into());
        if let Some(h) = &home {
            d.push(h.join("Library/Fonts"));
        }
    } else {
        d.push("/usr/share/fonts".into());
        d.push("/usr/local/share/fonts".into());
        if let Some(h) = &home {
            d.push(h.join(".local/share/fonts"));
            d.push(h.join(".fonts"));
        }
    }
    d
}

/// The family and whether it's the plain (regular) style, from a font file's names (only its
/// table list and names are read, not the whole file).
fn font_names(path: &std::path::Path) -> Option<(String, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    let n = u16::from_be_bytes([head[4], head[5]]) as usize;
    let mut dir = vec![0u8; n.min(64) * 16];
    f.read_exact(&mut dir).ok()?;
    let rec = dir.chunks_exact(16).find(|r| &r[0..4] == b"name")?;
    let off = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as u64;
    let len = (u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize).min(1 << 20);
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut data = vec![0u8; len];
    f.read_exact(&mut data).ok()?;
    let table = ttf_parser::name::Table::parse(&data)?;
    let name = |id: u16| table.names.into_iter().filter(|n| n.name_id == id).find_map(|n| n.to_string());
    let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))?;
    let style = name(ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY).or_else(|| name(ttf_parser::name_id::SUBFAMILY)).unwrap_or_default().to_lowercase();
    Some((family, matches!(style.as_str(), "regular" | "normal" | "book" | "roman" | "")))
}

/// The file of a font installed on this computer, by its family name ("Times New Roman"),
/// its plain style. Metric-compatible stand-ins are used where it's missing (Liberation Serif
/// for Times New Roman on Linux, say). Looked up once per name.
pub fn system_font(family: &str) -> Option<std::path::PathBuf> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static FOUND: Mutex<Option<HashMap<String, Option<std::path::PathBuf>>>> = Mutex::new(None);
    let key = family.trim().to_lowercase();
    if let Some(v) = FOUND.lock().ok()?.get_or_insert_with(HashMap::new).get(&key) {
        return v.clone();
    }
    let squash = |s: &str| s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let mut files = Vec::new();
    let mut stack = font_dirs();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if matches!(p.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase()).as_deref(), Some("ttf" | "otf")) {
                files.push(p);
            }
        }
    }
    let wanted: Vec<String> = std::iter::once(key.clone())
        .chain(match key.as_str() {
            "times new roman" | "times" | "serif" => vec!["times new roman", "liberation serif", "tinos", "dejavu serif"],
            "arial" | "helvetica" | "sans serif" | "sans-serif" => vec!["arial", "liberation sans", "arimo", "dejavu sans"],
            "courier new" | "courier" | "monospace" => vec!["courier new", "liberation mono", "cousine", "dejavu sans mono"],
            _ => vec![],
        }.into_iter().map(String::from))
        .collect();
    let mut found = None;
    'search: for w in &wanted {
        // Files named like the font first (times.ttf, timesnewroman.ttf...), then the rest.
        let sq = squash(w);
        let first = sq.chars().take(5).collect::<String>();
        let mut order: Vec<&std::path::PathBuf> = files.iter().filter(|p| p.file_stem().is_some_and(|s| squash(&s.to_string_lossy()).starts_with(&first))).collect();
        order.extend(files.iter().filter(|p| !p.file_stem().is_some_and(|s| squash(&s.to_string_lossy()).starts_with(&first))));
        let mut fallback = None;
        for p in order {
            if let Some((fam, regular)) = font_names(p) {
                if squash(&fam) == sq {
                    if regular {
                        found = Some(p.clone());
                        break 'search;
                    }
                    fallback.get_or_insert(p.clone());
                }
            }
        }
        if fallback.is_some() {
            found = fallback;
            break;
        }
    }
    FOUND.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, found.clone());
    found
}

#[cfg(test)]
mod system_font_tests {
    #[test]
    fn installed_fonts_are_found_by_name() {
        // On Linux with Liberation installed, Times New Roman's stand-in; elsewhere the real one.
        if let Some(p) = super::system_font("Times New Roman") {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            assert!(n.contains("times") || n.contains("liberationserif-regular") || n.contains("tinos"), "{}", n);
        }
        assert!(super::system_font("No Such Font Anywhere 123").is_none());
    }
}
