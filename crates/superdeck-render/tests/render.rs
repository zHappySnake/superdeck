// SPDX-License-Identifier: GPL-3.0-or-later

//! Renders keys and checks the result, against snapshots in
//! `tests/snapshots/` and by looking at pixels.
//!
//! To update the snapshots after an intended change, run with
//! `SUPERDECK_UPDATE_SNAPSHOTS=1` and look at every changed image.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use superdeck_render::{Error, Rendered, Renderer, TitleAlignment, TitleStyle};

const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 144 144">
  <rect width="144" height="144" fill="#2a6df4"/>
  <circle cx="72" cy="60" r="34" fill="#ffd23f"/>
  <text x="72" y="130" font-size="22" text-anchor="middle" fill="white">SVG</text>
</svg>"##;

const BLACK: Rgb<u8> = Rgb([0, 0, 0]);

fn renderer() -> Renderer {
    Renderer::with_bundled_fonts_only()
}

fn still(rendered: Rendered) -> RgbImage {
    match rendered {
        Rendered::Still(image) => image,
        Rendered::Animated(_) => panic!("expected a still image"),
    }
}

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut data = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut data), ImageFormat::Png)
        .unwrap();
    data
}

fn gif(frames: &[(Rgba<u8>, u32)]) -> Vec<u8> {
    let mut data = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut data);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for &(color, ms) in frames {
            let image = RgbaImage::from_pixel(8, 8, color);
            let frame = Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(ms, 1));
            encoder.encode_frame(frame).unwrap();
        }
    }
    data
}

fn close(a: Rgb<u8>, b: Rgb<u8>, tolerance: u8) -> bool {
    a.0.iter()
        .zip(b.0)
        .all(|(&x, y)| x.abs_diff(y) <= tolerance)
}

/// Rows that contain a bright pixel.
fn bright_rows(image: &RgbImage) -> Vec<u32> {
    (0..image.height())
        .filter(|&y| (0..image.width()).any(|x| image.get_pixel(x, y).0.iter().all(|&c| c > 200)))
        .collect()
}

fn snapshot_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.png"))
}

/// Compares with a stored snapshot, allowing for rounding differences
/// between CPU architectures.
fn assert_snapshot(name: &str, image: &RgbImage) {
    let path = snapshot_path(name);
    if std::env::var_os("SUPERDECK_UPDATE_SNAPSHOTS").is_some() {
        image.save(&path).unwrap();
        return;
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run with SUPERDECK_UPDATE_SNAPSHOTS=1 to create it",
                path.display()
            )
        })
        .to_rgb8();
    let differing = if expected.dimensions() == image.dimensions() {
        expected
            .pixels()
            .zip(image.pixels())
            .filter(|&(a, b)| !close(*a, *b, 8))
            .count()
    } else {
        usize::MAX
    };
    if differing > 0 {
        let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.actual.png"));
        image.save(&actual).unwrap();
        panic!(
            "{name} differs from {} in {differing} pixels; got {}",
            path.display(),
            actual.display()
        );
    }
}

#[test]
fn snapshots() {
    let mut renderer = renderer();
    let icon = renderer.load_image(ICON_SVG.as_bytes()).unwrap();
    let bottom = TitleStyle {
        alignment: TitleAlignment::Bottom,
        ..TitleStyle::default()
    };
    let fancy = TitleStyle {
        alignment: TitleAlignment::Top,
        bold: true,
        underline: true,
        color: Rgb([255, 255, 0]),
        ..TitleStyle::default()
    };
    let large_italic = TitleStyle {
        font_size: 20.0,
        italic: true,
        ..TitleStyle::default()
    };
    let cases = [
        ("svg-bottom-72", Some(&icon), "Mute", &bottom, 72),
        ("svg-bottom-96", Some(&icon), "Mute", &bottom, 96),
        (
            "multiline-top",
            Some(&icon),
            "Line one\nLine two",
            &fancy,
            72,
        ),
        (
            "overflow-middle",
            None,
            "Overflowing title",
            &large_italic,
            72,
        ),
    ];
    for (name, image, title, style, size) in cases {
        assert_snapshot(name, &still(renderer.render(image, title, style, size)));
    }
}

#[test]
fn empty_key_is_black() {
    let key = still(renderer().render(None, "", &TitleStyle::default(), 72));
    assert_eq!(key.dimensions(), (72, 72));
    assert!(key.pixels().all(|&p| p == BLACK));
}

#[test]
fn hidden_and_blank_titles_draw_nothing() {
    let mut renderer = renderer();
    let hidden = TitleStyle {
        visible: false,
        ..TitleStyle::default()
    };
    for (title, style) in [("Hidden", &hidden), (" \n ", &TitleStyle::default())] {
        let key = still(renderer.render(None, title, style, 72));
        assert!(key.pixels().all(|&p| p == BLACK), "{title:?}");
    }
}

#[test]
fn title_alignment() {
    let mut renderer = renderer();
    let mut rows = |alignment| {
        let style = TitleStyle {
            alignment,
            ..TitleStyle::default()
        };
        bright_rows(&still(renderer.render(None, "Title", &style, 72)))
    };
    let top = rows(TitleAlignment::Top);
    let middle = rows(TitleAlignment::Middle);
    let bottom = rows(TitleAlignment::Bottom);
    assert!(top.iter().all(|&y| y < 24), "{top:?}");
    assert!(middle.iter().all(|&y| (24..48).contains(&y)), "{middle:?}");
    assert!(bottom.iter().all(|&y| y >= 48), "{bottom:?}");
    assert!(!top.is_empty() && !middle.is_empty() && !bottom.is_empty());
}

#[test]
fn font_size_scales_with_the_key() {
    let mut renderer = renderer();
    let style = TitleStyle::default();
    let mut height = |size| {
        let rows = bright_rows(&still(renderer.render(None, "Hg", &style, size)));
        rows.last().unwrap() - rows.first().unwrap()
    };
    let (small, large) = (height(72), height(144));
    assert!(large.abs_diff(small * 2) <= 2, "{small} vs {large}");
}

#[test]
fn extreme_titles_are_capped() {
    let mut renderer = renderer();
    for font_size in [f32::NAN, f32::INFINITY, -3.0, 0.0] {
        let style = TitleStyle {
            font_size,
            ..TitleStyle::default()
        };
        let key = still(renderer.render(None, "Title", &style, 72));
        assert!(key.pixels().all(|&p| p == BLACK), "{font_size}");
    }
    let huge = TitleStyle {
        font_size: 1e9,
        ..TitleStyle::default()
    };
    renderer.render(None, "W", &huge, 72);
    renderer.render(None, &"long ".repeat(100_000), &TitleStyle::default(), 72);
}

#[test]
fn title_color() {
    let style = TitleStyle {
        font_size: 30.0,
        color: Rgb([255, 0, 0]),
        ..TitleStyle::default()
    };
    let key = still(renderer().render(None, "I", &style, 72));
    assert!(key.pixels().any(|&p| p == Rgb([255, 0, 0])));
    assert!(key.pixels().all(|p| p.0[1] == 0 && p.0[2] == 0));
}

#[test]
fn unknown_font_falls_back_to_the_default() {
    let mut renderer = renderer();
    let default = renderer.render(None, "Abc", &TitleStyle::default(), 72);
    let style = TitleStyle {
        font_family: Some("No Such Font".to_owned()),
        ..TitleStyle::default()
    };
    let fallback = renderer.render(None, "Abc", &style, 72);
    assert_eq!(default.first_frame(), fallback.first_frame());
}

#[test]
fn bundled_fonts_are_listed() {
    assert_eq!(renderer().font_families(), ["Noto Sans"]);
}

#[test]
fn rendering_is_repeatable() {
    let mut renderer = renderer();
    let icon = renderer.load_image(ICON_SVG.as_bytes()).unwrap();
    let render = |renderer: &mut Renderer| {
        renderer
            .render(Some(&icon), "Again", &TitleStyle::default(), 72)
            .first_frame()
            .clone()
    };
    assert_eq!(render(&mut renderer), render(&mut renderer));
}

#[test]
fn png_transparency_becomes_black() {
    // Left half transparent white, right half opaque red.
    let image = RgbaImage::from_fn(72, 72, |x, _| {
        if x < 36 {
            Rgba([255, 255, 255, 0])
        } else {
            Rgba([255, 0, 0, 255])
        }
    });
    let mut renderer = renderer();
    let image = renderer.load_image(&png(&image)).unwrap();
    let key = still(renderer.render(Some(&image), "", &TitleStyle::default(), 72));
    assert_eq!(*key.get_pixel(10, 36), BLACK);
    assert_eq!(*key.get_pixel(60, 36), Rgb([255, 0, 0]));
}

#[test]
fn images_are_scaled_to_fit_and_centered() {
    let wide = RgbaImage::from_pixel(400, 200, Rgba([0, 255, 0, 255]));
    let mut renderer = renderer();
    let image = renderer.load_image(&png(&wide)).unwrap();
    let key = still(renderer.render(Some(&image), "", &TitleStyle::default(), 96));
    assert!(close(*key.get_pixel(48, 48), Rgb([0, 255, 0]), 2));
    assert_eq!(*key.get_pixel(48, 4), BLACK);
    assert_eq!(*key.get_pixel(48, 92), BLACK);
    assert!(close(*key.get_pixel(1, 48), Rgb([0, 255, 0]), 2));
}

#[test]
fn svg_is_drawn_at_key_resolution() {
    let mut renderer = renderer();
    let icon = renderer.load_image(ICON_SVG.as_bytes()).unwrap();
    for size in [72, 96, 200] {
        let key = still(renderer.render(Some(&icon), "", &TitleStyle::default(), size));
        assert_eq!(key.dimensions(), (size, size));
        // Background in a corner, circle a bit above the center.
        assert!(close(*key.get_pixel(1, 1), Rgb([0x2a, 0x6d, 0xf4]), 2));
        assert!(close(
            *key.get_pixel(size / 2, size * 60 / 144),
            Rgb([0xff, 0xd2, 0x3f]),
            2
        ));
    }
}

#[test]
fn data_urls() {
    let mut renderer = renderer();
    let red = png(&RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])));
    let base64 = format!("data:image/png;base64,{}", base64_encode(&red));
    let raw_svg = format!("data:image/svg+xml;charset=utf8,{ICON_SVG}");
    let escaped_svg = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1 1'%3E%3Crect width='1' height='1' fill='%23ff0000'/%3E%3C/svg%3E";
    // Plugins sometimes send the wrong media type; the content decides.
    let mislabeled = format!("data:image/jpeg;base64,{}", base64_encode(&red));
    for url in [&base64, &raw_svg, escaped_svg, &mislabeled] {
        let image = renderer
            .load_data_url(url)
            .unwrap_or_else(|e| panic!("{url}: {e}"));
        let key = still(renderer.render(Some(&image), "", &TitleStyle::default(), 72));
        assert_ne!(*key.get_pixel(36, 36), BLACK, "{url}");
    }
    assert!(matches!(
        renderer.load_data_url("not a data url"),
        Err(Error::DataUrl(_))
    ));
    assert!(matches!(
        renderer.load_data_url("data:image/png;base64,@@@@"),
        Err(Error::DataUrl(_))
    ));
}

#[test]
fn animated_gif() {
    let data = gif(&[(Rgba([255, 0, 0, 255]), 200), (Rgba([0, 0, 255, 255]), 0)]);
    let mut renderer = renderer();
    let image = renderer.load_image(&data).unwrap();
    assert!(image.is_animated());
    assert_eq!(image.frame_count(), 2);

    let style = TitleStyle {
        alignment: TitleAlignment::Bottom,
        ..TitleStyle::default()
    };
    let Rendered::Animated(frames) = renderer.render(Some(&image), "Title", &style, 72) else {
        panic!("expected an animation");
    };
    assert_eq!(frames.len(), 2);
    assert!(close(
        *frames[0].image.get_pixel(36, 20),
        Rgb([255, 0, 0]),
        4
    ));
    assert!(close(
        *frames[1].image.get_pixel(36, 20),
        Rgb([0, 0, 255]),
        4
    ));
    assert_eq!(frames[0].delay, Duration::from_millis(200));
    // A 0 ms delay plays at 100 ms, as in browsers.
    assert_eq!(frames[1].delay, Duration::from_millis(100));
    // The title is on every frame.
    for frame in &frames {
        assert!(!bright_rows(&frame.image).is_empty());
    }
}

#[test]
fn single_frame_gif_is_still() {
    let data = gif(&[(Rgba([0, 255, 0, 255]), 100)]);
    let mut renderer = renderer();
    let image = renderer.load_image(&data).unwrap();
    assert!(!image.is_animated());
    let key = still(renderer.render(Some(&image), "", &TitleStyle::default(), 72));
    assert!(close(*key.get_pixel(36, 36), Rgb([0, 255, 0]), 4));
}

#[test]
fn bad_images_are_rejected() {
    let renderer = renderer();
    assert!(matches!(
        renderer.load_image(b"hello"),
        Err(Error::UnknownFormat)
    ));
    assert!(matches!(renderer.load_image(b"<svg"), Err(Error::Svg(_))));
    let mut truncated = png(&RgbaImage::new(8, 8));
    truncated.truncate(40);
    assert!(matches!(
        renderer.load_image(&truncated),
        Err(Error::Decode(_))
    ));
    let huge = png(&RgbaImage::new(superdeck_render::MAX_IMAGE_SIZE + 1, 1));
    assert!(matches!(renderer.load_image(&huge), Err(Error::Decode(_))));
}

#[test]
fn svg_cannot_read_files() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10">
        <image width="10" height="10" xlink:href="file:///etc/hostname"/>
        <image width="10" height="10" href="/usr/share/icons/hicolor/48x48/apps/firefox.png"/>
    </svg>"#;
    let mut renderer = renderer();
    let image = renderer.load_image(svg.as_bytes()).unwrap();
    let key = still(renderer.render(Some(&image), "", &TitleStyle::default(), 72));
    assert!(key.pixels().all(|&p| p == BLACK));
}

/// Standard base64, to avoid a dependency for the tests.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
