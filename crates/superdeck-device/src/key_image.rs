// SPDX-License-Identifier: GPL-3.0-or-later

//! Turns an arbitrary image into the bytes a device key expects: scaled to
//! the key size, rotated and mirrored for the panel, and encoded as BMP or
//! JPEG.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, Rgb, RgbImage};

use crate::Error;
use crate::model::Spec;

/// JPEG quality for key images. Keys are small, so this stays well under a
/// few dozen reports per key.
const JPEG_QUALITY: u8 = 95;

/// Image format a device decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// 24-bit uncompressed Windows bitmap.
    Bmp,
    /// Baseline JPEG.
    Jpeg,
}

/// Counter-clockwise rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// No rotation.
    None,
    /// 90° counter-clockwise.
    Ccw90,
    /// 180°.
    Ccw180,
    /// 270° counter-clockwise (90° clockwise).
    Ccw270,
}

/// Orientation fix applied to an upright image: rotate first, then mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    /// Rotation, applied first.
    pub rotation: Rotation,
    /// Mirror left to right.
    pub flip_horizontal: bool,
    /// Mirror top to bottom.
    pub flip_vertical: bool,
}

impl Transform {
    /// Applies the transform to an image.
    #[must_use]
    pub fn apply(&self, image: &RgbImage) -> RgbImage {
        let mut out = match self.rotation {
            Rotation::None => image.clone(),
            // imageops rotates clockwise.
            Rotation::Ccw90 => imageops::rotate270(image),
            Rotation::Ccw180 => imageops::rotate180(image),
            Rotation::Ccw270 => imageops::rotate90(image),
        };
        if self.flip_horizontal {
            imageops::flip_horizontal_in_place(&mut out);
        }
        if self.flip_vertical {
            imageops::flip_vertical_in_place(&mut out);
        }
        out
    }
}

/// Scales `image` to fit a `size`×`size` key, keeping its aspect ratio and
/// centering it on black. Transparent pixels are composited onto black.
#[must_use]
pub fn fit_to_key(image: &DynamicImage, size: u32) -> RgbImage {
    let rgb = flatten(image);
    if rgb.width() == size && rgb.height() == size {
        return rgb;
    }
    let (width, height) = (
        u64::from(rgb.width().max(1)),
        u64::from(rgb.height().max(1)),
    );
    let size64 = u64::from(size);
    let (new_width, new_height) = if width >= height {
        (size64, (height * size64).div_ceil(width))
    } else {
        ((width * size64).div_ceil(height), size64)
    };
    // Both are at most `size`.
    let new_width = u32::try_from(new_width.max(1)).unwrap_or(size);
    let new_height = u32::try_from(new_height.max(1)).unwrap_or(size);
    let scaled = imageops::resize(&rgb, new_width, new_height, FilterType::Lanczos3);

    let mut key = RgbImage::new(size, size);
    let x = i64::from((size - new_width) / 2);
    let y = i64::from((size - new_height) / 2);
    imageops::overlay(&mut key, &scaled, x, y);
    key
}

/// Converts to RGB, compositing any alpha channel onto black.
fn flatten(image: &DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let scale = |c: u8| u8::try_from(u16::from(c) * u16::from(a) / 255).expect("<= 255");
        Rgb([scale(r), scale(g), scale(b)])
    })
}

/// Encodes an upright image into the device's native key image format.
pub fn encode_key_image(spec: &Spec, image: &DynamicImage) -> Result<Vec<u8>, Error> {
    let key = fit_to_key(image, spec.key_size);
    let native = spec.transform.apply(&key);
    match spec.image_format {
        ImageFormat::Bmp => Ok(encode_bmp(&native)),
        ImageFormat::Jpeg => {
            let mut out = Vec::new();
            JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(&native)?;
            Ok(out)
        }
    }
}

/// Encodes a 24-bit bottom-up BMP with a 40-byte `BITMAPINFOHEADER`, the
/// variant the BMP devices accept.
fn encode_bmp(image: &RgbImage) -> Vec<u8> {
    const HEADER_LEN: u32 = 14 + 40;
    // 3780 pixels per meter = 96 DPI.
    const PIXELS_PER_METER: u32 = 3780;

    let (width, height) = image.dimensions();
    let row_len = (width * 3).next_multiple_of(4);
    let data_len = row_len * height;
    let file_len = HEADER_LEN + data_len;

    let mut out = Vec::with_capacity(file_len as usize);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_len.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&HEADER_LEN.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // no compression
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&PIXELS_PER_METER.to_le_bytes());
    out.extend_from_slice(&PIXELS_PER_METER.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // palette size, important colors

    for y in (0..height).rev() {
        let row_start = out.len();
        for x in 0..width {
            let [r, g, b] = image.get_pixel(x, y).0;
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(row_start + row_len as usize, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::Model;

    const RED: Rgb<u8> = Rgb([255, 0, 0]);
    const BLACK: Rgb<u8> = Rgb([0, 0, 0]);

    /// A square image, black except for a red top-left quadrant.
    fn marker(size: u32) -> RgbImage {
        RgbImage::from_fn(size, size, |x, y| {
            if x < size / 2 && y < size / 2 {
                RED
            } else {
                BLACK
            }
        })
    }

    fn is_red(pixel: Rgb<u8>) -> bool {
        pixel.0[0] > 200 && pixel.0[1] < 60 && pixel.0[2] < 60
    }

    fn red_corner(image: &RgbImage) -> (bool, bool) {
        let (w, h) = image.dimensions();
        let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)];
        let red: Vec<_> = corners
            .into_iter()
            .filter(|&(x, y)| is_red(*image.get_pixel(x, y)))
            .collect();
        assert_eq!(red.len(), 1, "exactly one red corner: {red:?}");
        (red[0].0 != 0, red[0].1 != 0)
    }

    #[test]
    fn transforms_move_the_marker() {
        let image = marker(8);
        let t = |rotation, flip_horizontal, flip_vertical| Transform {
            rotation,
            flip_horizontal,
            flip_vertical,
        };
        // (right?, bottom?)
        assert_eq!(
            red_corner(&t(Rotation::None, false, false).apply(&image)),
            (false, false)
        );
        assert_eq!(
            red_corner(&t(Rotation::None, true, true).apply(&image)),
            (true, true)
        );
        assert_eq!(
            red_corner(&t(Rotation::Ccw90, false, false).apply(&image)),
            (false, true)
        );
        assert_eq!(
            red_corner(&t(Rotation::Ccw180, false, false).apply(&image)),
            (true, true)
        );
        assert_eq!(
            red_corner(&t(Rotation::Ccw270, false, false).apply(&image)),
            (true, false)
        );
    }

    #[test]
    fn mini_transform_is_a_transpose() {
        let image = RgbImage::from_fn(5, 5, |x, y| Rgb([u8::try_from(x * 10 + y).unwrap(), 0, 0]));
        let native = Model::Mini.spec().transform.apply(&image);
        for (x, y, pixel) in native.enumerate_pixels() {
            assert_eq!(pixel, image.get_pixel(y, x));
        }
    }

    #[test]
    fn fit_keeps_aspect_ratio_and_centers() {
        let wide = DynamicImage::ImageRgb8(RgbImage::from_pixel(200, 100, RED));
        let key = fit_to_key(&wide, 72);
        assert_eq!(key.dimensions(), (72, 72));
        assert!(is_red(*key.get_pixel(36, 36)));
        assert_eq!(*key.get_pixel(36, 2), BLACK);
        assert_eq!(*key.get_pixel(36, 69), BLACK);
    }

    #[test]
    fn transparency_becomes_black() {
        let image = RgbaImage::from_fn(4, 4, |x, _| {
            if x < 2 {
                Rgba([255, 255, 255, 0])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let key = fit_to_key(&DynamicImage::ImageRgba8(image), 4);
        assert_eq!(*key.get_pixel(0, 0), BLACK);
        assert_eq!(*key.get_pixel(3, 0), Rgb([255, 255, 255]));
    }

    #[test]
    fn bmp_matches_the_expected_layout() {
        let spec = Model::Original.spec();
        let black = DynamicImage::ImageRgb8(RgbImage::new(72, 72));
        let bmp = encode_key_image(spec, &black).unwrap();
        assert_eq!(bmp.len(), 15606);
        #[rustfmt::skip]
        let header = [
            0x42, 0x4d, 0xf6, 0x3c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00,
            0x28, 0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x3c, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00,
            0xc4, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(bmp[..54], header);
        assert!(bmp[54..].iter().all(|&b| b == 0));
    }

    /// Decodes what we'd send and checks where the marker ended up.
    fn native_marker_corner(model: Model) -> (bool, bool) {
        let spec = model.spec();
        let data = encode_key_image(spec, &DynamicImage::ImageRgb8(marker(spec.key_size))).unwrap();
        let decoded = image::load_from_memory(&data).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (spec.key_size, spec.key_size));
        red_corner(&decoded)
    }

    #[test]
    fn encoded_images_have_the_native_orientation() {
        // Gen 2 and the original: rotated 180°.
        assert_eq!(native_marker_corner(Model::Mk2), (true, true));
        assert_eq!(native_marker_corner(Model::Xl), (true, true));
        assert_eq!(native_marker_corner(Model::Original), (true, true));
        // Mini: transposed, so the top-left marker stays top-left; the
        // asymmetric case is covered by `mini_transform_is_a_transpose`.
        assert_eq!(native_marker_corner(Model::Mini), (false, false));
    }

    #[test]
    fn bmp_pixels_are_bgr_bottom_up() {
        let mut image = RgbImage::new(2, 2);
        image.put_pixel(0, 0, Rgb([1, 2, 3]));
        let bmp = encode_bmp(&image);
        // 2 px * 3 bytes = 6, padded to 8 per row. Top row is stored last.
        assert_eq!(bmp.len(), 54 + 16);
        assert_eq!(bmp[54 + 8..54 + 11], [3, 2, 1]);
    }
}
