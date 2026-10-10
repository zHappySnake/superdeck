// SPDX-License-Identifier: GPL-3.0-or-later

//! Images plugins name by path, turned into `data:` URLs.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::Engine;

use crate::manifest::{Plugin, RelativePath};

/// Largest image file read: the message size limit.
pub(crate) const MAX_IMAGE: u64 = 16 * 1024 * 1024;

/// The image of a `setImage`: a `data:` URL as it is, or a path inside the
/// plugin directory read into one.
pub(crate) fn resolve(plugin: &Plugin, image: &str) -> Result<String, String> {
    if image
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
    {
        return Ok(image.to_owned());
    }
    let path = RelativePath::new(image)?;
    let file = plugin
        .image_file(&path)
        .ok_or_else(|| format!("there's no image {path} in the plugin directory"))?;
    // Symbolic links could lead out of the plugin directory.
    let real = file.canonicalize().map_err(|e| e.to_string())?;
    let dir = plugin.dir.canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&dir) {
        return Err(format!("{path} leads outside the plugin directory"));
    }
    data_url(&real)
}

/// Reads an image file into a `data:` URL, its type recognized from the
/// content.
pub(crate) fn data_url(file: &Path) -> Result<String, String> {
    let mut data = Vec::new();
    File::open(file)
        .and_then(|f| f.take(MAX_IMAGE + 1).read_to_end(&mut data))
        .map_err(|e| format!("could not read {}: {e}", file.display()))?;
    if data.len() as u64 > MAX_IMAGE {
        return Err(format!("{} is larger than 16 MiB", file.display()));
    }
    let mime = mime_type(&data).ok_or_else(|| {
        format!(
            "{} is not a PNG, JPEG, GIF, BMP, or SVG image",
            file.display()
        )
    })?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&data)
    ))
}

fn mime_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        let text = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
        text.trim_ascii_start()
            .starts_with(b"<")
            .then_some("image/svg+xml")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn plugin(dir: &Path) -> Plugin {
        let plugin_dir = dir.join("com.example.a.sdPlugin");
        fs::create_dir_all(plugin_dir.join("imgs")).unwrap();
        fs::write(
            plugin_dir.join("manifest.json"),
            r#"{ "Name": "A", "Version": "1", "Actions": [] }"#,
        )
        .unwrap();
        fs::write(plugin_dir.join("imgs/on.svg"), "<svg/>").unwrap();
        fs::write(plugin_dir.join("imgs/note.txt"), "hello").unwrap();
        fs::write(dir.join("secret.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.svg"), plugin_dir.join("imgs/link.svg"))
            .unwrap();
        Plugin::load(&plugin_dir).unwrap()
    }

    #[test]
    fn paths_become_data_urls() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = plugin(temp.path());
        assert_eq!(
            resolve(&plugin, "data:image/png;base64,AAAA"),
            Ok("data:image/png;base64,AAAA".to_owned())
        );
        assert_eq!(
            resolve(&plugin, "imgs/on"),
            Ok("data:image/svg+xml;base64,PHN2Zy8+".to_owned())
        );
        assert_eq!(
            resolve(&plugin, "imgs/on.svg"),
            Ok("data:image/svg+xml;base64,PHN2Zy8+".to_owned())
        );
        assert!(resolve(&plugin, "imgs/missing").is_err());
        assert!(resolve(&plugin, "../secret.svg").is_err());
        assert!(resolve(&plugin, "/etc/passwd").is_err());
        assert!(
            resolve(&plugin, "imgs/link.svg")
                .unwrap_err()
                .contains("outside")
        );
        assert!(data_url(&plugin.dir.join("imgs/note.txt")).is_err());
    }
}
