// SPDX-License-Identifier: GPL-3.0-or-later

//! Key titles: text laid out with `cosmic-text` and drawn with a dark
//! outline so it stays readable on any image.

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, LegacyRenderer, Metrics, PhysicalGlyph,
    Renderer, Shaping, Style, SwashCache, UnderlineStyle, Weight, Wrap, render_decoration,
};
use image::Rgb;
use resvg::tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::compose::{blend, mul, over};
use crate::fonts::DEFAULT_FONT_FAMILY;

/// Key size that title font sizes and margins are given for. Other key sizes
/// scale them.
pub const REFERENCE_KEY_SIZE: f32 = 72.0;

/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.2;

/// Space between a top or bottom aligned title and the key edge, at the
/// reference key size.
const EDGE_MARGIN: f32 = 2.0;

/// Width of the dark outline around the text, at the reference key size.
const OUTLINE_WIDTH: f32 = 1.0;

/// Largest font size, at the reference key size. Plugins set titles, so
/// sizes and lengths are capped to keep rendering cheap.
pub const MAX_FONT_SIZE: f32 = 144.0;

/// Longest title, in bytes; the rest is dropped.
pub const MAX_TITLE_LEN: usize = 1024;

/// Where a title sits on the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleAlignment {
    /// Along the top edge.
    Top,
    /// Centered vertically.
    #[default]
    Middle,
    /// Along the bottom edge.
    Bottom,
}

/// How a title looks: the title settings of the Stream Deck SDK
/// (`titleParameters`).
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent toggles, as in the SDK"
)]
pub struct TitleStyle {
    /// Whether to draw the title at all.
    pub visible: bool,
    /// Font family; `None`, or one that isn't installed, means the bundled
    /// Noto Sans.
    pub font_family: Option<String>,
    /// Font size in pixels on a 72-pixel key, scaled for other key sizes.
    pub font_size: f32,
    /// Bold weight.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Vertical position.
    pub alignment: TitleAlignment,
    /// Text color.
    pub color: Rgb<u8>,
}

impl Default for TitleStyle {
    fn default() -> Self {
        Self {
            visible: true,
            font_family: None,
            font_size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            alignment: TitleAlignment::default(),
            color: Rgb([255, 255, 255]),
        }
    }
}

/// Draws `text` onto a transparent `size`×`size` layer: lines centered
/// horizontally, not wrapped, clipped at the key edges. Returns `None` if
/// there's nothing to draw.
pub(crate) fn layer(
    fonts: &mut FontSystem,
    glyphs: &mut SwashCache,
    text: &str,
    style: &TitleStyle,
    size: u32,
) -> Option<Pixmap> {
    let text = truncate(text, MAX_TITLE_LEN);
    let size_ok = style.font_size.is_finite() && style.font_size > 0.0;
    if !style.visible || !size_ok || text.trim().is_empty() {
        return None;
    }
    #[expect(clippy::cast_precision_loss, reason = "key sizes are small")]
    let key = size as f32;
    let scale = key / REFERENCE_KEY_SIZE;
    let font_size = style.font_size.min(MAX_FONT_SIZE) * scale;

    let family = style
        .font_family
        .as_deref()
        .filter(|name| has_family(fonts, name))
        .unwrap_or(DEFAULT_FONT_FAMILY);
    let attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(if style.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        })
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .underline(if style.underline {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        });

    let mut buffer = Buffer::new(fonts, Metrics::new(font_size, font_size * LINE_HEIGHT));
    buffer.set_wrap(Wrap::None);
    // No width, so lines aren't wrapped; each line is centered on the key
    // when drawn, and one wider than the key overflows evenly on both sides.
    buffer.set_size(None, None);
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(fonts, false);

    let height = buffer
        .layout_runs()
        .map(|run| run.line_top + run.line_height)
        .fold(0.0, f32::max);
    let margin = EDGE_MARGIN * scale;
    let top = match style.alignment {
        TitleAlignment::Top => margin,
        TitleAlignment::Middle => (key - height) / 2.0,
        TitleAlignment::Bottom => key - margin - height,
    };

    let mut text_layer = Pixmap::new(size, size)?;
    let [r, g, b] = style.color.0;
    let color = Color::rgb(r, g, b);
    for run in buffer.layout_runs() {
        let left = (key - run.line_w) / 2.0;
        let mut renderer = LegacyRenderer {
            font_system: fonts,
            cache: glyphs,
            callback: |x, y, w, h, color| fill(&mut text_layer, x, y, w, h, color),
        };
        for glyph in run.glyphs {
            let physical = glyph.physical((left, top + run.line_y), 1.0);
            renderer.glyph(physical, glyph.color_opt.unwrap_or(color));
        }
        // Decorations are positioned relative to the run; shift them the
        // same way as the glyphs.
        let mut shifted = Shifted {
            inner: &mut renderer,
            dx: round(left),
            dy: round(top),
        };
        render_decoration(&mut shifted, &run, color);
    }

    let radius = OUTLINE_WIDTH * scale;
    let mut out = outline(&text_layer, radius);
    over(&mut out, &text_layer);
    Some(out)
}

/// The longest prefix of `text` that fits in `max` bytes.
fn truncate(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[expect(clippy::cast_possible_truncation, reason = "near the key")]
fn round(value: f32) -> i32 {
    value.round() as i32
}

/// Moves rectangles drawn through it by a whole number of pixels.
struct Shifted<'a, R> {
    inner: &'a mut R,
    dx: i32,
    dy: i32,
}

impl<R: Renderer> Renderer for Shifted<'_, R> {
    fn rectangle(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        self.inner.rectangle(x + self.dx, y + self.dy, w, h, color);
    }

    fn glyph(&mut self, physical_glyph: PhysicalGlyph, color: Color) {
        self.inner.glyph(physical_glyph, color);
    }
}

/// Whether the font database has a face with this family name.
fn has_family(fonts: &FontSystem, name: &str) -> bool {
    fonts
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
}

/// Blends a solid rectangle of straight-alpha `color` over the layer,
/// clipped to its bounds.
fn fill(layer: &mut Pixmap, x: i32, y: i32, w: u32, h: u32, color: Color) {
    let (width, height) = (layer.width(), layer.height());
    let clip = |start: i32, len: u32, max: u32| {
        let start = i64::from(start);
        let end = (start + i64::from(len)).min(i64::from(max));
        let start = start.max(0);
        Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?)).filter(|(s, e)| s < e)
    };
    let (Some((x0, x1)), Some((y0, y1))) = (clip(x, w, width), clip(y, h, height)) else {
        return;
    };
    let [red, green, blue, alpha] = color.as_rgba();
    let src = [mul(red, alpha), mul(green, alpha), mul(blue, alpha), alpha];
    let stride = width as usize;
    let pixels = layer.pixels_mut();
    for y in y0..y1 {
        for pixel in &mut pixels[y * stride + x0..y * stride + x1] {
            *pixel = blend(src, *pixel);
        }
    }
}

/// A black layer around the text, `radius` pixels wide. Coverage fades
/// over the last pixel of the radius, so a fractional radius gives a
/// lighter outline instead of jumping to the next whole pixel.
fn outline(text: &Pixmap, radius: f32) -> Pixmap {
    let (width, height) = (text.width() as usize, text.height() as usize);
    let alpha: Vec<u8> = text.pixels().iter().map(|p| p.alpha()).collect();

    // Weight of each neighbor offset, by its distance.
    #[expect(clippy::cast_possible_truncation, reason = "a few pixels")]
    let reach = radius.ceil() as isize;
    let mut kernel = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            #[expect(clippy::cast_precision_loss, reason = "a few pixels")]
            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if weight > 0.0 {
                kernel.push((dx, dy, weight));
            }
        }
    }

    let mut out = Pixmap::new(text.width(), text.height()).expect("same size as the text");
    let pixels = out.pixels_mut();
    for y in 0..height {
        for x in 0..width {
            let strongest = kernel
                .iter()
                .filter_map(|&(dx, dy, weight)| {
                    let sx = x.checked_add_signed(dx).filter(|&sx| sx < width)?;
                    let sy = y.checked_add_signed(dy).filter(|&sy| sy < height)?;
                    Some(f32::from(alpha[sy * width + sx]) * weight)
                })
                .fold(0.0, f32::max);
            #[expect(clippy::cast_possible_truncation, reason = "0 to 255")]
            #[expect(clippy::cast_sign_loss, reason = "0 to 255")]
            let strongest = strongest.round() as u8;
            pixels[y * width + x] =
                PremultipliedColorU8::from_rgba(0, 0, 0, strongest).expect("black is valid");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_whole_characters() {
        assert_eq!(truncate("héllo", 2), "h");
        assert_eq!(truncate("héllo", 3), "hé");
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn fill_clips_to_the_layer() {
        let mut layer = Pixmap::new(4, 4).unwrap();
        fill(&mut layer, -2, 3, 10, 5, Color::rgba(255, 255, 255, 255));
        let opaque: Vec<_> = layer.pixels().iter().map(|p| p.alpha() == 255).collect();
        assert_eq!(opaque.iter().filter(|&&o| o).count(), 4);
        assert!(opaque[12..].iter().all(|&o| o));
    }
}
