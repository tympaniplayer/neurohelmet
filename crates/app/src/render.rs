// Neurohelmet — Copyright (C) 2026 Nate Palmer
//
// This file is part of Neurohelmet.
//
// Neurohelmet is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// Neurohelmet is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR
// A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// Neurohelmet. If not, see <https://www.gnu.org/licenses/>.

//! Rasterize a ratatui cell buffer into an RGB image, for the game-log export. Each terminal cell
//! becomes an 8×8 glyph (optionally upscaled) blitted through the `font8x8` bitmap font and colored
//! by the cell's fg/bg. Pure pixels in, PPM out — no terminal needed.

use font8x8::{UnicodeFonts, BASIC_FONTS, BLOCK_FONTS, BOX_FONTS, LATIN_FONTS};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// An RGB image (row-major, 3 bytes/pixel).
pub struct Image {
    pub w: usize,
    pub h: usize,
    px: Vec<[u8; 3]>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Self {
        Image {
            w,
            h,
            px: vec![[0, 0, 0]; w * h],
        }
    }

    fn put(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = rgb;
        }
    }

    /// Copy another image's pixels in with the top-left at `(ox, oy)`.
    pub fn blit(&mut self, src: &Image, ox: usize, oy: usize) {
        for y in 0..src.h {
            for x in 0..src.w {
                self.put(ox + x, oy + y, src.px[y * src.w + x]);
            }
        }
    }

    /// Write binary PPM (P6).
    pub fn write_ppm(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = io::BufWriter::new(File::create(path)?);
        write!(f, "P6\n{} {}\n255\n", self.w, self.h)?;
        let mut bytes = Vec::with_capacity(self.w * self.h * 3);
        for p in &self.px {
            bytes.extend_from_slice(p);
        }
        f.write_all(&bytes)
    }

    /// Write a PNG (8-bit RGB) — what GitHub renders inline.
    pub fn write_png(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::with_capacity(self.w * self.h * 3);
        for p in &self.px {
            bytes.extend_from_slice(p);
        }
        let file = io::BufWriter::new(File::create(path)?);
        let mut enc = png::Encoder::new(file, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(io::Error::other)?;
        writer.write_image_data(&bytes).map_err(io::Error::other)
    }
}

const CELL: usize = 8; // glyph size in pixels (font8x8)

/// Rasterize a buffer at `scale`× (each cell → 8·scale square).
pub fn rasterize(buf: &Buffer, scale: usize) -> Image {
    let cols = buf.area.width as usize;
    let rows = buf.area.height as usize;
    let mut img = Image::new(cols * CELL * scale, rows * CELL * scale);
    for cy in 0..rows {
        for cx in 0..cols {
            let cell = &buf[(cx as u16, cy as u16)];
            let mut fg = rgb(cell.fg, [0xCC, 0xCC, 0xCC]);
            let mut bg = rgb(cell.bg, [0, 0, 0]);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = cell.symbol().chars().next().unwrap_or(' ');
            let g = glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..CELL {
                    let color = if bits & (1 << col) != 0 { fg } else { bg };
                    let px0 = (cx * CELL + col) * scale;
                    let py0 = (cy * CELL + row) * scale;
                    for sy in 0..scale {
                        for sx in 0..scale {
                            img.put(px0 + sx, py0 + sy, color);
                        }
                    }
                }
            }
        }
    }
    img
}

/// 8×8 bitmap for a glyph: font8x8 (ASCII + box-drawing + block + latin-1), then our hand-drawn
/// extras for the geometric/symbol glyphs the font lacks, else blank. Row 0 is the top; within a
/// row, bit `i` (value `1<<i`) is column `i` from the left.
fn glyph(ch: char) -> [u8; 8] {
    BASIC_FONTS
        .get(ch)
        .or_else(|| BOX_FONTS.get(ch))
        .or_else(|| BLOCK_FONTS.get(ch))
        .or_else(|| LATIN_FONTS.get(ch))
        .or_else(|| extra(ch))
        .unwrap_or([0; 8])
}

// Hand-drawn extras (LSB = leftmost column, row 0 = top).
const TRI_LEFT: [u8; 8] = [0x40, 0x60, 0x70, 0x78, 0x78, 0x70, 0x60, 0x40];
const TRI_RIGHT: [u8; 8] = [0x02, 0x06, 0x0E, 0x1E, 0x1E, 0x0E, 0x06, 0x02];
const TRI_UP: [u8; 8] = [0x00, 0x18, 0x3C, 0x3C, 0x7E, 0x7E, 0xFF, 0x00];
const TRI_DOWN: [u8; 8] = [0x00, 0xFF, 0x7E, 0x7E, 0x3C, 0x3C, 0x18, 0x00];
const CIRCLE: [u8; 8] = [0x00, 0x3C, 0x7E, 0x7E, 0x7E, 0x7E, 0x3C, 0x00];
const CHECK: [u8; 8] = [0x00, 0x80, 0x40, 0x20, 0x12, 0x0C, 0x00, 0x00];
const DASH: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00];
const DOTS: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x49];

fn extra(ch: char) -> Option<[u8; 8]> {
    Some(match ch {
        '—' => DASH,
        '…' => DOTS,
        '←' | '◀' => TRI_LEFT,
        '→' | '▶' | '▸' => TRI_RIGHT,
        '↑' | '▲' | '⚠' => TRI_UP,
        '↓' | '▼' => TRI_DOWN,
        '●' => CIRCLE,
        '✓' => CHECK,
        _ => return None,
    })
}

/// Map a ratatui color to RGB (VS Code terminal palette). `reset` is used for `Color::Reset`.
fn rgb(c: Color, reset: [u8; 3]) -> [u8; 3] {
    match c {
        Color::Reset => reset,
        Color::Black => [0, 0, 0],
        Color::Red => [205, 49, 49],
        Color::Green => [13, 188, 121],
        Color::Yellow => [229, 229, 16],
        Color::Blue => [36, 114, 200],
        Color::Magenta => [188, 63, 188],
        Color::Cyan => [17, 168, 205],
        Color::Gray => [170, 170, 170],
        Color::DarkGray => [102, 102, 102],
        Color::LightRed => [241, 76, 76],
        Color::LightGreen => [35, 209, 139],
        Color::LightYellow => [245, 245, 67],
        Color::LightBlue => [59, 142, 234],
        Color::LightMagenta => [214, 112, 214],
        Color::LightCyan => [41, 184, 219],
        Color::White => [229, 229, 229],
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Indexed(i) => indexed(i),
    }
}

/// Standard xterm 256-color → RGB (16 system + 6×6×6 cube + 24 greys).
fn indexed(i: u8) -> [u8; 3] {
    match i {
        0..=15 => {
            const SYS: [Color; 16] = [
                Color::Black,
                Color::Red,
                Color::Green,
                Color::Yellow,
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::Gray,
                Color::DarkGray,
                Color::LightRed,
                Color::LightGreen,
                Color::LightYellow,
                Color::LightBlue,
                Color::LightMagenta,
                Color::LightCyan,
                Color::White,
            ];
            rgb(SYS[i as usize], [0, 0, 0])
        }
        16..=231 => {
            let n = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [step(n / 36), step((n / 6) % 6), step(n % 6)]
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            [v, v, v]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn known_glyphs_are_nonblank() {
        for ch in ['A', '0', '█', '─', '▶', '✓', '●'] {
            assert_ne!(glyph(ch), [0; 8], "glyph {ch:?} should not be blank");
        }
        assert_eq!(glyph('\u{0000}'), [0; 8], "unknown glyph is blank");
    }

    #[test]
    fn rasterizes_cell_colors_and_ppm_header() {
        // One green 'A' on black + one reversed cell.
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        buf[(0, 0)]
            .set_symbol("A")
            .set_style(Style::default().fg(Color::Green));
        buf[(1, 0)]
            .set_symbol(" ")
            .set_style(Style::default().bg(Color::Red));
        let img = rasterize(&buf, 1); // 2 cells × 8px = 16×8
        assert_eq!((img.w, img.h), (16, 8));
        // 'A' top row in font8x8 has set pixels somewhere in cell 0 → at least one green pixel.
        assert!(
            img.px.contains(&[13, 188, 121]),
            "the green A should produce green pixels"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.ppm");
        img.write_ppm(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"P6\n16 8\n255\n"), "PPM header");
        assert_eq!(bytes.len(), "P6\n16 8\n255\n".len() + 16 * 8 * 3);
    }
}
