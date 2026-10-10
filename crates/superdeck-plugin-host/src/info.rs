// SPDX-License-Identifier: GPL-3.0-or-later

//! The registration info: the `-info` argument of plugins and the `inInfo`
//! of property inspectors, as in `registration-info.schema.json`.

use serde_json::{Value, json};

use crate::message::Device;

/// The version of the official app whose protocol SuperDeck implements, as
/// plugins are told it.
pub const COMPATIBLE_VERSION: &str = "7.1.0";

/// Languages the official app supports, which plugins may have
/// translations for.
const LANGUAGES: [&str; 8] = ["de", "en", "es", "fr", "ja", "ko", "zh_CN", "zh_TW"];

/// About the app and the system, for plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// One of the official app's languages.
    pub language: String,
    /// The app's font family.
    pub font: String,
    /// The kernel release.
    pub platform_version: String,
}

impl AppInfo {
    /// The user's language and the running kernel.
    #[must_use]
    pub fn from_system() -> Self {
        Self {
            language: language(sys_locale::get_locale().as_deref()).to_owned(),
            font: "Noto Sans".to_owned(),
            platform_version: std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .map(|release| release.trim().to_owned())
                .unwrap_or_default(),
        }
    }
}

/// The official app's language for a BCP 47 locale such as `de-AT` or
/// `zh-Hant-TW`; English for the rest.
fn language(locale: Option<&str>) -> &'static str {
    let Some(locale) = locale else { return "en" };
    let mut parts = locale.split(['-', '_', '.', '@']);
    let primary = parts.next().unwrap_or("").to_ascii_lowercase();
    if primary == "zh" {
        let rest: Vec<String> = parts.map(str::to_ascii_lowercase).collect();
        let traditional = rest
            .iter()
            .any(|part| ["hant", "tw", "hk", "mo"].contains(&part.as_str()));
        return if traditional { "zh_TW" } else { "zh_CN" };
    }
    LANGUAGES
        .iter()
        .find(|language| **language == primary)
        .copied()
        .unwrap_or("en")
}

/// The registration info for a plugin.
pub(crate) fn registration_info(
    app: &AppInfo,
    plugin_uuid: &str,
    plugin_version: &str,
    devices: &[Device],
) -> Value {
    json!({
        "application": {
            "font": app.font,
            "language": app.language,
            "platform": "linux",
            "platformVersion": app.platform_version,
            "version": COMPATIBLE_VERSION,
        },
        "plugin": { "uuid": plugin_uuid, "version": plugin_version },
        "devicePixelRatio": 1,
        "colors": {
            "buttonMouseOverBackgroundColor": "#464646",
            "buttonPressedBackgroundColor": "#303030",
            "buttonPressedBorderColor": "#646464",
            "buttonPressedTextColor": "#969696",
            "highlightColor": "#0078ff",
        },
        "devices": devices.iter().map(|device| json!({
            "id": device.id,
            "name": device.info.name,
            "size": device.info.size,
            "type": device.info.kind,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages() {
        for (locale, expected) in [
            (None, "en"),
            (Some("en-US"), "en"),
            (Some("de-AT"), "de"),
            (Some("fr_CA.UTF-8"), "fr"),
            (Some("pt-BR"), "en"),
            (Some("zh-CN"), "zh_CN"),
            (Some("zh-Hans"), "zh_CN"),
            (Some("zh-Hant-TW"), "zh_TW"),
            (Some("zh-HK"), "zh_TW"),
            (Some("ja"), "ja"),
            (Some("C"), "en"),
        ] {
            assert_eq!(language(locale), expected, "{locale:?}");
        }
    }
}
