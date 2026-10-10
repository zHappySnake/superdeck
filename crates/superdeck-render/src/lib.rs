// SPDX-License-Identifier: GPL-3.0-or-later

//! Key image composition: turns a key's image and title into the bitmap a
//! device key shows.
//!
//! A [`Renderer`] loads images ([`KeyImage`]: PNG, JPEG, BMP, still or
//! animated GIF, SVG, from bytes or a `data:` URL) and renders them at any
//! key size with a title on top. Output is an upright RGB image on black;
//! `superdeck-device` scales, rotates, and encodes it for the device.
//!
//! ```
//! use superdeck_render::{Renderer, TitleAlignment, TitleStyle};
//!
//! let mut renderer = Renderer::with_bundled_fonts_only();
//! let style = TitleStyle {
//!     alignment: TitleAlignment::Bottom,
//!     ..TitleStyle::default()
//! };
//! let key = renderer.render(None, "Mute", &style, 72);
//! assert_eq!(key.first_frame().dimensions(), (72, 72));
//! ```
//!
//! Rendering is deterministic for a given set of fonts, so tests use
//! [`Renderer::with_bundled_fonts_only`].

mod compose;
mod error;
mod fonts;
mod key_image;
mod title;

use std::sync::Arc;
use std::time::Duration;

use cosmic_text::{FontSystem, SwashCache};
use image::{RgbImage, RgbaImage};
use resvg::tiny_skia::{self, Pixmap, PremultipliedColorU8};
use resvg::usvg;

pub use error::Error;
pub use fonts::DEFAULT_FONT_FAMILY;
pub use key_image::{KeyImage, MAX_ANIMATION_PIXELS, MAX_IMAGE_SIZE, MAX_STORED_SIZE};
pub use title::{MAX_FONT_SIZE, MAX_TITLE_LEN, REFERENCE_KEY_SIZE, TitleAlignment, TitleStyle};

use key_image::Content;

/// A rendered key.
#[derive(Debug, Clone)]
pub enum Rendered {
    /// A single image.
    Still(RgbImage),
    /// Frames to show in turn, looping.
    Animated(Vec<AnimationFrame>),
}

/// One frame of a rendered animation.
#[derive(Debug, Clone)]
pub struct AnimationFrame {
    /// The key image.
    pub image: RgbImage,
    /// How long to show it.
    pub delay: Duration,
}

impl Rendered {
    /// The still image, or an animation's first frame.
    #[must_use]
    pub fn first_frame(&self) -> &RgbImage {
        match self {
            Rendered::Still(image) => image,
            Rendered::Animated(frames) => &frames[0].image,
        }
    }
}

/// Loads key images and renders keys. Holds the font database, so create one
/// and reuse it: loading the installed fonts takes a moment.
pub struct Renderer {
    fonts: FontSystem,
    glyphs: SwashCache,
    svg_options: usvg::Options<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("font_faces", &self.fonts.db().len())
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// A renderer with the bundled fonts and every installed font.
    #[must_use]
    pub fn new() -> Self {
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
        Self::with_database(fonts::database(true), locale)
    }

    /// A renderer with only the bundled fonts, which renders the same on
    /// every machine. Characters Noto Sans lacks show as missing glyphs.
    #[must_use]
    pub fn with_bundled_fonts_only() -> Self {
        Self::with_database(fonts::database(false), "en-US".to_owned())
    }

    fn with_database(db: usvg::fontdb::Database, locale: String) -> Self {
        let svg_options = usvg::Options {
            font_family: DEFAULT_FONT_FAMILY.to_owned(),
            fontdb: Arc::new(db.clone()),
            // Images inside an SVG may be data URLs, but never files: an
            // SVG from a plugin has no business reading the disk.
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
                resolve_string: Box::new(|_, _| None),
            },
            ..usvg::Options::default()
        };
        Self {
            fonts: FontSystem::new_with_locale_and_db(locale, db),
            glyphs: SwashCache::new(),
            svg_options,
        }
    }

    /// Family names of every available font, sorted, for a font picker.
    #[must_use]
    pub fn font_families(&self) -> Vec<String> {
        let mut families: Vec<String> = self
            .fonts
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        families.sort_unstable();
        families.dedup();
        families
    }

    /// Loads an image file's contents. The format is recognized from the
    /// data.
    pub fn load_image(&self, data: &[u8]) -> Result<KeyImage, Error> {
        key_image::decode(data, &self.svg_options)
    }

    /// Loads an image from a `data:` URL, as sent by a plugin's `setImage`:
    /// base64 (`data:image/png;base64,…`) or not (`data:image/svg+xml,<svg…`).
    pub fn load_data_url(&self, url: &str) -> Result<KeyImage, Error> {
        key_image::decode_data_url(url, &self.svg_options)
    }

    /// Renders a `size`×`size` key: `image` scaled to fit and centered on
    /// black, with `title` drawn over it in `style`.
    ///
    /// # Panics
    ///
    /// If `size` is 0.
    pub fn render(
        &mut self,
        image: Option<&KeyImage>,
        title: &str,
        style: &TitleStyle,
        size: u32,
    ) -> Rendered {
        assert!(size > 0, "key size must be positive");
        let title = title::layer(&mut self.fonts, &mut self.glyphs, title, style, size);
        let compose = |draw_image: &dyn Fn(&mut Pixmap)| {
            let mut canvas = Pixmap::new(size, size).expect("size is positive");
            canvas.fill(tiny_skia::Color::BLACK);
            draw_image(&mut canvas);
            if let Some(title) = &title {
                compose::over(&mut canvas, title);
            }
            to_rgb(&canvas)
        };
        match image.map(|image| &image.0) {
            None => Rendered::Still(compose(&|_| {})),
            Some(Content::Raster(raster)) => {
                Rendered::Still(compose(&|canvas| draw_raster(canvas, raster)))
            }
            Some(Content::Svg(tree)) => Rendered::Still(compose(&|canvas| draw_svg(canvas, tree))),
            Some(Content::Animated(frames)) => Rendered::Animated(
                frames
                    .iter()
                    .map(|frame| AnimationFrame {
                        image: compose(&|canvas| draw_raster(canvas, &frame.image)),
                        delay: frame.delay,
                    })
                    .collect(),
            ),
        }
    }
}

/// Draws a premultiplied image scaled to fit and centered.
fn draw_raster(canvas: &mut Pixmap, image: &RgbaImage) {
    let size = canvas.width();
    let (width, height) = key_image::fit(image.width(), image.height(), size);
    let scaled;
    let image = if image.dimensions() == (width, height) {
        image
    } else {
        scaled = key_image::resize_premultiplied(image, width, height);
        &scaled
    };
    let mut layer = Pixmap::new(size, size).expect("size is positive");
    let (x0, y0) = ((size - width) / 2, (size - height) / 2);
    let stride = size as usize;
    let pixels = layer.pixels_mut();
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        pixels[(y0 + y) as usize * stride + (x0 + x) as usize] =
            PremultipliedColorU8::from_rgba(r, g, b, a).expect("stored premultiplied");
    }
    compose::over(canvas, &layer);
}

/// Draws an SVG scaled to fit and centered.
fn draw_svg(canvas: &mut Pixmap, tree: &usvg::Tree) {
    #[expect(clippy::cast_precision_loss, reason = "key sizes are small")]
    let size = canvas.width() as f32;
    let svg = tree.size();
    let scale = (size / svg.width()).min(size / svg.height());
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (size - svg.width() * scale) / 2.0,
        (size - svg.height() * scale) / 2.0,
    );
    resvg::render(tree, transform, &mut canvas.as_mut());
}

/// Converts the opaque canvas to RGB.
fn to_rgb(canvas: &Pixmap) -> RgbImage {
    let pixels = canvas.pixels();
    RgbImage::from_fn(canvas.width(), canvas.height(), |x, y| {
        let pixel = pixels[(y * canvas.width() + x) as usize].demultiply();
        image::Rgb([pixel.red(), pixel.green(), pixel.blue()])
    })
}
