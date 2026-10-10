// SPDX-License-Identifier: GPL-3.0-or-later

/// Errors from loading key images.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The data isn't PNG, JPEG, GIF, BMP, or SVG.
    #[error("unrecognized image format")]
    UnknownFormat,

    /// A raster image failed to decode, or is over the size limits.
    #[error("could not decode image: {0}")]
    Decode(#[from] image::ImageError),

    /// An SVG image failed to parse.
    #[error("could not parse SVG: {0}")]
    Svg(#[from] resvg::usvg::Error),

    /// A `data:` URL is malformed.
    #[error("invalid data URL: {0}")]
    DataUrl(String),

    /// An animated GIF is longer than
    /// [`MAX_ANIMATION_PIXELS`](crate::MAX_ANIMATION_PIXELS) allows.
    #[error("animation is too long")]
    AnimationTooLarge,
}
