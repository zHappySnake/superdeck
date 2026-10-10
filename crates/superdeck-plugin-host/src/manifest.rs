// SPDX-License-Identifier: GPL-3.0-or-later

//! Plugin manifests (`manifest.json`), as described in
//! `sdk/protocol/README.md` and `sdk/protocol/schemas/manifest.schema.json`.
//!
//! Manifests are written for the official app, so reading is lenient:
//! unknown fields are ignored, and fields that mean nothing on Linux (`OS`,
//! `CodePathMac`, `CodePathWin`, `ApplicationsToMonitor`) aren't read.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, de};

use crate::Error;

/// The manifest's file name in a plugin directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Suffix of plugin directory names.
pub const PLUGIN_DIR_SUFFIX: &str = ".sdPlugin";

/// A plugin's `manifest.json`: the fields SuperDeck uses.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Manifest {
    /// Plugin UUID. Older manifests leave it out; the directory name has it.
    #[serde(rename = "UUID", default)]
    pub uuid: Option<String>,
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Author's name.
    #[serde(default)]
    pub author: Option<String>,
    /// What the plugin does.
    #[serde(default)]
    pub description: Option<String>,
    /// The plugin's website.
    #[serde(rename = "URL", default)]
    pub url: Option<String>,
    /// Icon for the plugin list, without its extension.
    #[serde(default)]
    pub icon: Option<RelativePath>,
    /// Heading of the plugin's group in the action list.
    #[serde(default)]
    pub category: Option<String>,
    /// Icon of that heading, without its extension.
    #[serde(default)]
    pub category_icon: Option<RelativePath>,
    /// The plugin's entry point.
    #[serde(default)]
    pub code_path: Option<RelativePath>,
    /// Present for Node.js plugins.
    #[serde(default)]
    pub nodejs: Option<Nodejs>,
    /// SDK version the plugin was written for.
    #[serde(rename = "SDKVersion", default)]
    pub sdk_version: Option<u32>,
    /// Version of the official app the plugin needs.
    #[serde(default)]
    pub software: Option<Software>,
    /// Property inspector for actions that don't name their own.
    #[serde(default)]
    pub property_inspector_path: Option<RelativePath>,
    /// The plugin's actions.
    pub actions: Vec<ActionManifest>,
    /// Profiles the plugin ships.
    #[serde(default)]
    pub profiles: Vec<ProfileManifest>,
}

/// The `Nodejs` section of a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Nodejs {
    /// Version asked for; ignored, every plugin runs on Node.js 24 or newer.
    #[serde(default)]
    pub version: Option<String>,
    /// Debugging: `"enabled"`, `"break"`, or Node.js arguments.
    #[serde(default)]
    pub debug: Option<String>,
}

/// The `Software` section of a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Software {
    /// Version of the official app the plugin needs.
    #[serde(default)]
    pub minimum_version: Option<String>,
}

/// A profile a plugin ships.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct ProfileManifest {
    /// Path of the `.streamDeckProfile`, without its extension; also the
    /// name `switchToProfile` uses.
    pub name: String,
    /// SDK device type the profile is for.
    pub device_type: u32,
}

/// An action in a manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
#[expect(clippy::struct_excessive_bools, reason = "the manifest's fields")]
pub struct ActionManifest {
    /// Action UUID.
    #[serde(rename = "UUID")]
    pub uuid: String,
    /// Name in the action list.
    pub name: String,
    /// Icon in the action list, without its extension.
    #[serde(default)]
    pub icon: Option<RelativePath>,
    /// Tooltip in the action list.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// The action's property inspector.
    #[serde(default)]
    pub property_inspector_path: Option<RelativePath>,
    /// One or two states.
    pub states: Vec<StateManifest>,
    /// Controllers the action works with; `None` means keys only.
    #[serde(default)]
    pub controllers: Option<Vec<String>>,
    /// Don't toggle a two-state action when its key is released.
    #[serde(default)]
    pub disable_automatic_states: bool,
    /// Whether the action can be a Multi Action step.
    #[serde(default = "yes")]
    pub supported_in_multi_actions: bool,
    /// Whether the action is in the action list.
    #[serde(default = "yes")]
    pub visible_in_actions_list: bool,
    /// Whether the user may set a title.
    #[serde(default = "yes")]
    pub user_title_enabled: bool,
}

fn yes() -> bool {
    true
}

impl ActionManifest {
    /// Whether the action works on keys.
    #[must_use]
    pub fn on_keys(&self) -> bool {
        self.controllers
            .as_ref()
            .is_none_or(|controllers| controllers.iter().any(|c| c == "Keypad"))
    }
}

/// How a state looks until the plugin or the user changes it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct StateManifest {
    /// Key image, without its extension.
    #[serde(default)]
    pub image: Option<RelativePath>,
    /// Image in a Multi Action's step list, without its extension.
    #[serde(default)]
    pub multi_action_image: Option<RelativePath>,
    /// State name, shown when choosing a Multi Action step's state.
    #[serde(default)]
    pub name: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Whether the title is shown.
    #[serde(default)]
    pub show_title: Option<bool>,
    /// `top`, `middle`, or `bottom`.
    #[serde(default)]
    pub title_alignment: Option<String>,
    /// `#rrggbb`.
    #[serde(default)]
    pub title_color: Option<String>,
    /// Font family.
    #[serde(default)]
    pub font_family: Option<String>,
    /// Font size; some manifests write it as a string.
    #[serde(default, deserialize_with = "lenient_number")]
    pub font_size: Option<f64>,
    /// `Regular`, `Bold`, `Italic`, `Bold Italic`, or empty.
    #[serde(default)]
    pub font_style: Option<String>,
    /// Underlined title.
    #[serde(default)]
    pub font_underline: Option<bool>,
}

fn lenient_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberOrString {
        Number(f64),
        String(String),
    }
    match Option::<NumberOrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumberOrString::Number(n)) => Ok(Some(n)),
        Some(NumberOrString::String(s)) => s
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| de::Error::custom(format!("invalid number {s:?}"))),
    }
}

/// A path inside the plugin directory, as written in the manifest: relative,
/// without `..`. Backslashes, which Windows-minded manifests use, count as
/// separators.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelativePath(String);

impl RelativePath {
    /// Checks `path`.
    pub fn new(path: &str) -> Result<Self, String> {
        let normalized = path.replace('\\', "/");
        let error = || format!("{path:?} must be a relative path inside the plugin directory");
        if normalized.is_empty() || normalized.starts_with('/') {
            return Err(error());
        }
        for component in Path::new(&normalized).components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(error()),
            }
        }
        if normalized.as_bytes().get(1) == Some(&b':') {
            // A Windows drive letter.
            return Err(error());
        }
        Ok(Self(normalized))
    }

    /// The path, with `/` as separator.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path inside `dir`.
    #[must_use]
    pub fn within(&self, dir: &Path) -> PathBuf {
        dir.join(&self.0)
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for RelativePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// How a plugin runs, or why it can't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A Node.js script.
    Node {
        /// The script.
        script: PathBuf,
        /// `Nodejs.Debug` from the manifest.
        debug: Option<String>,
    },
    /// The plugin can't run on SuperDeck yet.
    Unsupported(String),
}

/// A plugin found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    /// Plugin UUID.
    pub uuid: String,
    /// The `<uuid>.sdPlugin` directory.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
    /// How it runs.
    pub entry: Entry,
}

impl Plugin {
    /// Reads the plugin in `dir`, a `<uuid>.sdPlugin` directory.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read(&path).map_err(Error::io(&path))?;
        // Some editors on Windows write a byte order mark.
        let text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&text);
        let manifest: Manifest = serde_json::from_slice(text).map_err(|source| Error::Json {
            path: path.clone(),
            source,
        })?;
        let invalid = |reason: String| Error::InvalidManifest {
            path: path.clone(),
            reason,
        };

        let dir_name = dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let dir_uuid = dir_name.strip_suffix(PLUGIN_DIR_SUFFIX).ok_or_else(|| {
            invalid(format!(
                "the directory name must end in {PLUGIN_DIR_SUFFIX}"
            ))
        })?;
        let uuid = manifest.uuid.clone().unwrap_or_else(|| dir_uuid.to_owned());
        if uuid != dir_uuid {
            return Err(invalid(format!(
                "UUID {uuid:?} doesn't match the directory name {dir_name:?}"
            )));
        }
        check_uuid(&uuid).map_err(invalid)?;
        if manifest.name.trim().is_empty() {
            return Err(invalid("Name is empty".to_owned()));
        }
        let mut seen = HashSet::new();
        for action in &manifest.actions {
            check_uuid(&action.uuid).map_err(invalid)?;
            if !seen.insert(action.uuid.as_str()) {
                return Err(invalid(format!("action {:?} is listed twice", action.uuid)));
            }
            if !(1..=2).contains(&action.states.len()) {
                return Err(invalid(format!(
                    "action {:?} has {} states; it must have one or two",
                    action.uuid,
                    action.states.len()
                )));
            }
        }

        let entry = entry(dir, &manifest);
        Ok(Self {
            uuid,
            dir: dir.to_owned(),
            manifest,
            entry,
        })
    }

    /// The action with this UUID.
    #[must_use]
    pub fn action(&self, uuid: &str) -> Option<&ActionManifest> {
        self.manifest
            .actions
            .iter()
            .find(|action| action.uuid == uuid)
    }

    /// The file of an image the manifest names without its extension: the
    /// first of `<path>.svg`, `<path>@2x.png`, `<path>.png`, and
    /// `<path>.gif` that exists. A path that already has one of those
    /// extensions is used as it is.
    #[must_use]
    pub fn image_file(&self, image: &RelativePath) -> Option<PathBuf> {
        let base = image.within(&self.dir);
        let has_extension = Path::new(image.as_str())
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ["svg", "png", "gif", "jpg", "jpeg"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            });
        if has_extension {
            return base.is_file().then_some(base);
        }
        ["svg", "@2x.png", "png", "gif"]
            .iter()
            .map(|suffix| {
                let mut name = base.clone().into_os_string();
                if !suffix.starts_with('@') {
                    name.push(".");
                }
                name.push(suffix);
                PathBuf::from(name)
            })
            .find(|path| path.is_file())
    }
}

/// UUIDs are reverse-DNS names: at least two dot-separated parts of
/// letters, digits, `-`, and `_`.
fn check_uuid(uuid: &str) -> Result<(), String> {
    let part_ok = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if uuid.contains('.') && uuid.split('.').all(part_ok) {
        Ok(())
    } else {
        Err(format!(
            "{uuid:?} is not a valid UUID; expected reverse-DNS form such as com.example.plugin"
        ))
    }
}

fn entry(dir: &Path, manifest: &Manifest) -> Entry {
    let Some(code_path) = &manifest.code_path else {
        return Entry::Unsupported("the manifest has no CodePath for Linux".to_owned());
    };
    let extension = Path::new(code_path.as_str())
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match (extension.as_deref(), &manifest.nodejs) {
        (Some("js" | "mjs" | "cjs"), Some(nodejs)) => {
            let script = code_path.within(dir);
            if script.is_file() {
                Entry::Node {
                    script,
                    debug: nodejs.debug.clone(),
                }
            } else {
                Entry::Unsupported(format!("its CodePath {code_path} doesn't exist"))
            }
        }
        (Some("html" | "htm"), _) => Entry::Unsupported(
            "plugins written as web pages will run in the desktop app".to_owned(),
        ),
        (Some("exe"), _) => {
            Entry::Unsupported("it's a Windows program, which can't run on Linux".to_owned())
        }
        _ => Entry::Unsupported(
            "only Node.js plugins run on SuperDeck; this one has no Nodejs section".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_stay_inside() {
        for ok in ["imgs/key", "bin\\plugin.js", "./a", "a/./b"] {
            assert!(RelativePath::new(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            RelativePath::new("bin\\plugin.js").unwrap().as_str(),
            "bin/plugin.js"
        );
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../b",
            "a/..",
            "C:\\x",
            "\\\\server\\x",
        ] {
            assert!(RelativePath::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn uuids_are_reverse_dns() {
        assert!(check_uuid("com.example.counter").is_ok());
        assert!(check_uuid("com.Example.my_plugin-2").is_ok());
        for bad in [
            "counter",
            "com..x",
            ".com.x",
            "com.x.",
            "com.ex ample",
            "com/x",
        ] {
            assert!(check_uuid(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn font_size_may_be_a_string() {
        let state: StateManifest =
            serde_json::from_value(serde_json::json!({ "FontSize": "12" })).unwrap();
        assert_eq!(state.font_size, Some(12.0));
        let state: StateManifest =
            serde_json::from_value(serde_json::json!({ "FontSize": 9.5 })).unwrap();
        assert_eq!(state.font_size, Some(9.5));
        assert!(
            serde_json::from_value::<StateManifest>(serde_json::json!({ "FontSize": "big" }))
                .is_err()
        );
    }
}
