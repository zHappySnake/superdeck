// SPDX-License-Identifier: GPL-3.0-or-later

//! Loading key images: PNG, JPEG, BMP, still and animated GIF, and SVG, from
//! bytes or a `data:` URL.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::imageops::{self, FilterType};
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage};
use resvg::usvg;

use crate::Error;

/// Largest width or height a decoded image may have.
pub const MAX_IMAGE_SIZE: u32 = 4096;

/// Loaded images are scaled down to fit this size, twice the largest key
/// (Stream Deck Studio), so a long animation doesn't hold full-size frames.
pub const MAX_STORED_SIZE: u32 = 288;

/// Total pixels, over all frames, an animation may keep after scaling down:
/// 256 frames at [`MAX_STORED_SIZE`], about 85 MB.
pub const MAX_ANIMATION_PIXELS: u64 = 288 * 288 * 256;

/// Browsers show GIF frames with a shorter delay than this for
/// [`SHORT_DELAY_REPLACEMENT`] instead, and so do we.
const MIN_FRAME_DELAY: Duration = Duration::from_millis(20);
const SHORT_DELAY_REPLACEMENT: Duration = Duration::from_millis(100);

/// A decoded key image, ready to render at any key size. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct KeyImage(pub(crate) Content);

#[derive(Debug, Clone)]
pub(crate) enum Content {
    /// Premultiplied RGBA.
    Raster(Arc<RgbaImage>),
    Svg(Arc<usvg::Tree>),
    Animated(Arc<[Frame]>),
}

/// One frame of an animation.
#[derive(Debug)]
pub(crate) struct Frame {
    /// Premultiplied RGBA.
    pub image: RgbaImage,
    pub delay: Duration,
}

impl KeyImage {
    /// Whether this is an animation with more than one frame.
    #[must_use]
    pub fn is_animated(&self) -> bool {
        matches!(self.0, Content::Animated(_))
    }

    /// Number of frames: 1 unless animated.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        match &self.0 {
            Content::Animated(frames) => frames.len(),
            _ => 1,
        }
    }
}

/// Decodes image bytes, recognizing the format from the content.
pub(crate) fn decode(data: &[u8], svg_options: &usvg::Options) -> Result<KeyImage, Error> {
    match image::guess_format(data) {
        Ok(ImageFormat::Gif) => decode_gif(data),
        Ok(format) => decode_raster(data, format),
        Err(_) if looks_like_svg(data) => {
            let tree = usvg::Tree::from_data(data, svg_options)?;
            Ok(KeyImage(Content::Svg(Arc::new(tree))))
        }
        Err(_) => Err(Error::UnknownFormat),
    }
}

/// Decodes a `data:` URL, as plugins send to `setImage`. The media type is
/// ignored in favor of the content, since plugins sometimes get it wrong.
///
/// Plugins often send SVG unescaped (`data:image/svg+xml,<svg fill="#f00"…`),
/// so `#` is taken as part of the data, not the start of a fragment.
pub(crate) fn decode_data_url(url: &str, svg_options: &usvg::Options) -> Result<KeyImage, Error> {
    let url = url.replace('#', "%23");
    let url = data_url::DataUrl::process(&url).map_err(|e| Error::DataUrl(format!("{e:?}")))?;
    let (data, _fragment) = url
        .decode_to_vec()
        .map_err(|_| Error::DataUrl("invalid base64".to_owned()))?;
    decode(&data, svg_options)
}

/// SVG is XML, optionally gzipped (`.svgz`).
fn looks_like_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    data.starts_with(&[0x1f, 0x8b]) || data.trim_ascii_start().starts_with(b"<")
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIZE);
    limits.max_image_height = Some(MAX_IMAGE_SIZE);
    limits
}

fn decode_raster(data: &[u8], format: ImageFormat) -> Result<KeyImage, Error> {
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    reader.limits(limits());
    let image = reader.decode()?.into_rgba8();
    Ok(KeyImage(Content::Raster(Arc::new(prepare(image)))))
}

fn decode_gif(data: &[u8]) -> Result<KeyImage, Error> {
    let mut decoder = GifDecoder::new(Cursor::new(data))?;
    decoder.set_limits(limits())?;
    let mut frames = Vec::new();
    let mut pixels = 0;
    for frame in decoder.into_frames() {
        let frame = frame?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay = Duration::from_secs_f64(f64::from(numer) / f64::from(denom.max(1)) / 1000.0);
        let delay = if delay < MIN_FRAME_DELAY {
            SHORT_DELAY_REPLACEMENT
        } else {
            delay
        };
        let image = prepare(frame.into_buffer());
        pixels += u64::from(image.width()) * u64::from(image.height());
        if pixels > MAX_ANIMATION_PIXELS {
            return Err(Error::AnimationTooLarge);
        }
        frames.push(Frame { image, delay });
    }
    match frames.len() {
        0 => Err(Error::UnknownFormat),
        1 => Ok(KeyImage(Content::Raster(Arc::new(
            frames.pop().expect("one frame").image,
        )))),
        _ => Ok(KeyImage(Content::Animated(frames.into()))),
    }
}

/// Premultiplies and scales down to [`MAX_STORED_SIZE`].
fn prepare(mut image: RgbaImage) -> RgbaImage {
    premultiply(&mut image);
    if image.width().max(image.height()) <= MAX_STORED_SIZE {
        return image;
    }
    let (width, height) = fit(image.width(), image.height(), MAX_STORED_SIZE);
    resize_premultiplied(&image, width, height)
}

fn premultiply(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let scale = |c: u8| u8::try_from(u16::from(c) * u16::from(a) / 255).expect("<= 255");
        pixel.0 = [scale(r), scale(g), scale(b), a];
    }
}

/// Resizes a premultiplied image. Lanczos can overshoot, so color channels
/// are clamped to alpha to stay valid premultiplied values.
pub(crate) fn resize_premultiplied(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut out = imageops::resize(image, width, height, FilterType::Lanczos3);
    for pixel in out.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        pixel.0 = [r.min(a), g.min(a), b.min(a), a];
    }
    out
}

/// Size of a `width`×`height` image scaled to fit a `size`×`size` square,
/// keeping its aspect ratio.
pub(crate) fn fit(width: u32, height: u32, size: u32) -> (u32, u32) {
    let (width, height, size) = (
        u64::from(width.max(1)),
        u64::from(height.max(1)),
        u64::from(size),
    );
    let (w, h) = if width >= height {
        (size, (height * size).div_ceil(width))
    } else {
        ((width * size).div_ceil(height), size)
    };
    // Both are at most `size`, which came from a u32.
    let w = u32::try_from(w.max(1)).expect("fits");
    let h = u32::try_from(h.max(1)).expect("fits");
    (w, h)
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn fit_keeps_aspect_ratio() {
        assert_eq!(fit(200, 100, 72), (72, 36));
        assert_eq!(fit(100, 200, 72), (36, 72));
        assert_eq!(fit(10, 10, 72), (72, 72));
        assert_eq!(fit(1000, 1, 72), (72, 1));
    }

    #[test]
    fn premultiplied_resize_stays_valid() {
        // A sharp edge between opaque white and transparent: Lanczos rings.
        let image = RgbaImage::from_fn(64, 64, |x, _| {
            if x < 32 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let out = resize_premultiplied(&image, 13, 13);
        for pixel in out.pixels() {
            let [r, g, b, a] = pixel.0;
            assert!(r <= a && g <= a && b <= a, "{pixel:?}");
        }
    }

    #[test]
    fn svg_sniffing() {
        assert!(looks_like_svg(b"  \n<svg/>"));
        assert!(looks_like_svg(b"\xef\xbb\xbf<?xml version=\"1.0\"?>"));
        assert!(looks_like_svg(&[0x1f, 0x8b, 0x08]));
        assert!(!looks_like_svg(b"hello"));
    }
}
