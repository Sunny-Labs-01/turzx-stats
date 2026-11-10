//! A 1920x480 RGBA picture with the few shapes the screen needs, and its way to the panel:
//! rotated into the 480x1920 framebuffer and packed as an RGBA PNG.

use std::collections::HashMap;

pub const W: usize = 1920;
pub const H: usize = 480;
/// The bezel hides the top 18 rows in landscape.
pub const HIDDEN_TOP: i32 = 18;

pub type Rgb = (u8, u8, u8);

pub struct Fonts {
    regular: fontdue::Font,
    bold: fontdue::Font,
    cache: HashMap<(char, u32, bool), (fontdue::Metrics, Vec<u8>)>,
}

impl Fonts {
    pub fn load(dir: &str) -> Result<Fonts, String> {
        let load = |file: &str| -> Result<fontdue::Font, String> {
            let path = format!("{dir}/{file}");
            let bytes = std::fs::read(&path).map_err(|e| format!("font {path}: {e}"))?;
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| format!("font {path}: {e}"))
        };
        Ok(Fonts {
            regular: load("DejaVuSans.ttf")?,
            bold: load("DejaVuSans-Bold.ttf")?,
            cache: HashMap::new(),
        })
    }

    fn glyph(&mut self, ch: char, px: u32, bold: bool) -> &(fontdue::Metrics, Vec<u8>) {
        let font = if bold { &self.bold } else { &self.regular };
        self.cache
            .entry((ch, px, bold))
            .or_insert_with(|| font.rasterize(ch, px as f32))
    }

    pub fn width(&mut self, text: &str, px: u32, bold: bool) -> i32 {
        text.chars()
            .map(|c| self.glyph(c, px, bold).0.advance_width)
            .sum::<f32>()
            .ceil() as i32
    }

    /// The largest size from `px` down at which `text` is no wider than `max`.
    pub fn fit(&mut self, text: &str, px: u32, bold: bool, max: i32) -> u32 {
        let mut size = px;
        while size > 12 && self.width(text, size, bold) > max {
            size -= 2;
        }
        size
    }
}

pub struct Canvas {
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(bg: Rgb) -> Canvas {
        let mut px = Vec::with_capacity(W * H * 4);
        for _ in 0..W * H {
            px.extend_from_slice(&[bg.0, bg.1, bg.2, 255]);
        }
        Canvas { px }
    }

    fn blend(&mut self, x: i32, y: i32, c: Rgb, alpha: u32) {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 || alpha == 0 {
            return;
        }
        let i = (y as usize * W + x as usize) * 4;
        for (k, v) in [c.0, c.1, c.2].into_iter().enumerate() {
            let old = self.px[i + k] as u32;
            self.px[i + k] = ((v as u32 * alpha + old * (255 - alpha)) / 255) as u8;
        }
    }

    /// A filled rectangle with corners of radius `r`.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgb) {
        let r = r.min(w / 2).min(h / 2).max(0);
        for py in 0..h {
            for px in 0..w {
                let dx = if px < r { r - px } else if px >= w - r { px - (w - r - 1) } else { 0 };
                let dy = if py < r { r - py } else if py >= h - r { py - (h - r - 1) } else { 0 };
                let alpha = if dx > 0 && dy > 0 {
                    let d = ((dx * dx + dy * dy) as f32).sqrt() - r as f32;
                    ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32
                } else {
                    255
                };
                self.blend(x + px, y + py, c, alpha);
            }
        }
    }

    /// Text with its baseline at `y`, starting at `x`. Returns the width drawn.
    pub fn text(&mut self, f: &mut Fonts, x: i32, y: i32, px: u32, bold: bool, c: Rgb, s: &str) -> i32 {
        let mut pen = x as f32;
        for ch in s.chars() {
            let (m, bitmap) = f.glyph(ch, px, bold).clone();
            let gx = pen.round() as i32 + m.xmin;
            let gy = y - m.ymin - m.height as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bitmap[row * m.width + col] as u32;
                    self.blend(gx + col as i32, gy + row as i32, c, a);
                }
            }
            pen += m.advance_width;
        }
        (pen - x as f32).ceil() as i32
    }

    pub fn text_right(&mut self, f: &mut Fonts, right: i32, y: i32, px: u32, bold: bool, c: Rgb, s: &str) {
        let w = f.width(s, px, bold);
        self.text(f, right - w, y, px, bold, c, s);
    }

    /// A track with its filled part, `frac` 0..1.
    pub fn bar(&mut self, x: i32, y: i32, w: i32, h: i32, frac: f32, track: Rgb, fill: Rgb) {
        self.rect(x, y, w, h, h / 2, track);
        let filled = (w as f32 * frac.clamp(0.0, 1.0)).round() as i32;
        if filled > 0 {
            self.rect(x, y, filled.max(h), h, h / 2, fill);
        }
    }

    /// Columns for a history of values 0..100, the newest at the right edge.
    pub fn history(&mut self, x: i32, y: i32, w: i32, h: i32, values: &[f32], track: Rgb, fill: Rgb) {
        self.rect(x, y + h - 2, w, 2, 0, track);
        let step = 5;
        let slots = (w / step) as usize;
        let shown = &values[values.len().saturating_sub(slots)..];
        let start = x + w - shown.len() as i32 * step;
        for (i, v) in shown.iter().enumerate() {
            let bar = ((h - 2) as f32 * (v / 100.0).clamp(0.0, 1.0)).round() as i32;
            if bar > 0 {
                self.rect(start + i as i32 * step, y + h - 2 - bar, step - 2, bar, 0, fill);
            }
        }
    }

    /// The panel's framebuffer is 480x1920: the landscape picture turned a quarter clockwise.
    pub fn to_png(&self) -> Vec<u8> {
        let mut native = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in 0..W {
                let src = (y * W + x) * 4;
                let dst = (x * H + (H - 1 - y)) * 4;
                native[dst..dst + 4].copy_from_slice(&self.px[src..src + 4]);
            }
        }
        encode_png(&native, H as u32, W as u32)
    }

    /// The picture as it is drawn, upright, for looking at on a desk.
    pub fn to_landscape_png(&self) -> Vec<u8> {
        encode_png(&self.px, W as u32, H as u32)
    }
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        // RGBA is a must: with RGB the panel's decoder is off by a byte per pixel
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    out
}
