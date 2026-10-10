// SPDX-License-Identifier: GPL-3.0-or-later

//! Blending premultiplied pixels.

use resvg::tiny_skia::{Pixmap, PremultipliedColorU8};

/// Draws `top` over `bottom`; both premultiplied and the same size.
pub(crate) fn over(bottom: &mut Pixmap, top: &Pixmap) {
    for (dst, src) in bottom.pixels_mut().iter_mut().zip(top.pixels()) {
        let s = [src.red(), src.green(), src.blue(), src.alpha()];
        *dst = blend(s, *dst);
    }
}

/// Source-over for premultiplied colors.
pub(crate) fn blend(src: [u8; 4], dst: PremultipliedColorU8) -> PremultipliedColorU8 {
    let keep = 255 - src[3];
    let c = |s: u8, d: u8| s.saturating_add(mul(d, keep));
    let a = c(src[3], dst.alpha());
    let channel = |s, d| c(s, d).min(a);
    PremultipliedColorU8::from_rgba(
        channel(src[0], dst.red()),
        channel(src[1], dst.green()),
        channel(src[2], dst.blue()),
        a,
    )
    .expect("channels clamped to alpha")
}

/// `a * b / 255`, rounded.
pub(crate) fn mul(a: u8, b: u8) -> u8 {
    let product = u16::from(a) * u16::from(b) + 128;
    u8::try_from((product + (product >> 8)) >> 8).expect("<= 255")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_rounds() {
        assert_eq!(mul(255, 255), 255);
        assert_eq!(mul(255, 0), 0);
        assert_eq!(mul(128, 255), 128);
        assert_eq!(mul(128, 128), 64);
    }

    #[test]
    fn blend_is_source_over() {
        let dst = PremultipliedColorU8::from_rgba(0, 0, 200, 255).unwrap();
        let opaque = blend([255, 0, 0, 255], dst);
        assert_eq!((opaque.red(), opaque.blue(), opaque.alpha()), (255, 0, 255));
        let clear = blend([0, 0, 0, 0], dst);
        assert_eq!(clear, dst);
        let half = blend([128, 0, 0, 128], dst);
        assert_eq!((half.red(), half.blue(), half.alpha()), (128, 100, 255));
    }
}
