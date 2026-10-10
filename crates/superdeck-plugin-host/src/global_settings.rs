// SPDX-License-Identifier: GPL-3.0-or-later

//! Plugins' global settings, in `<config>/plugins/<uuid>/settings.json`.
//!
//! The file follows the conventions of SuperDeck's other files: a format
//! version, pretty-printed JSON, atomic replacement, and a damaged file
//! moved aside rather than overwritten. It's readable only by the user,
//! since plugins keep secrets there.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::message::Settings;

/// Format version of `settings.json`.
pub const GLOBAL_SETTINGS_VERSION: u64 = 1;

const FILE: &str = "settings.json";

/// What can go wrong; the message is for the plugin's log.
pub(crate) type Problem = String;

/// Global settings of every plugin, read when first needed.
#[derive(Debug)]
pub(crate) struct GlobalSettings {
    /// `<config>/plugins`.
    root: PathBuf,
    loaded: HashMap<String, Entry>,
}

#[derive(Debug)]
struct Entry {
    settings: Settings,
    /// Saving is refused, so as not to destroy a newer version's file.
    read_only: bool,
}

impl GlobalSettings {
    /// Settings kept under `root`, usually `$XDG_CONFIG_HOME/superdeck/plugins`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            loaded: HashMap::new(),
        }
    }

    fn file(&self, plugin: &str) -> PathBuf {
        self.root.join(plugin).join(FILE)
    }

    /// A plugin's settings. Problems reading them are returned with empty
    /// settings.
    pub fn get(&mut self, plugin: &str) -> (Settings, Option<Problem>) {
        if let Some(entry) = self.loaded.get(plugin) {
            return (entry.settings.clone(), None);
        }
        let path = self.file(plugin);
        let (entry, problem) = load(&path);
        let settings = entry.settings.clone();
        self.loaded.insert(plugin.to_owned(), entry);
        (settings, problem)
    }

    /// Replaces a plugin's settings.
    pub fn set(&mut self, plugin: &str, settings: Settings) -> Result<(), Problem> {
        // Read first, so a newer version's file is noticed.
        let (_, problem) = self.get(plugin);
        let path = self.file(plugin);
        let entry = self.loaded.get_mut(plugin).expect("just loaded");
        if entry.read_only {
            return Err(problem.unwrap_or_else(|| {
                format!(
                    "not saving global settings: {} was written by a newer SuperDeck",
                    path.display()
                )
            }));
        }
        entry.settings = settings;
        let mut text = serde_json::to_string_pretty(&json!({
            "formatVersion": GLOBAL_SETTINGS_VERSION,
            "settings": entry.settings,
        }))
        .expect("JSON values serialize");
        text.push('\n');
        save(&path, text.as_bytes())
            .map_err(|e| format!("could not save global settings to {}: {e}", path.display()))
    }
}

fn load(path: &Path) -> (Entry, Option<Problem>) {
    let empty = |read_only| Entry {
        settings: Settings::new(),
        read_only,
    };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (empty(false), None),
        Err(e) => {
            return (
                empty(true),
                Some(format!("could not read {}: {e}", path.display())),
            );
        }
    };
    let reason = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(mut map)) => match map.remove("formatVersion") {
            Some(Value::Number(n)) if n.as_u64() == Some(GLOBAL_SETTINGS_VERSION) => {
                match (map.remove("settings"), map.is_empty()) {
                    (Some(Value::Object(settings)), true) => {
                        return (
                            Entry {
                                settings,
                                read_only: false,
                            },
                            None,
                        );
                    }
                    (None, true) => return (empty(false), None),
                    _ => "unexpected fields".to_owned(),
                }
            }
            Some(Value::Number(n)) if n.as_u64().is_some_and(|v| v > GLOBAL_SETTINGS_VERSION) => {
                return (
                    empty(true),
                    Some(format!(
                        "{} was written by a newer SuperDeck (format version {n}); its settings aren't used or replaced",
                        path.display()
                    )),
                );
            }
            _ => "no valid formatVersion".to_owned(),
        },
        Ok(_) => "not a JSON object".to_owned(),
        Err(e) => e.to_string(),
    };
    let problem = match set_aside(path) {
        Ok(moved_to) => format!(
            "{} is damaged ({reason}); it was moved to {}",
            path.display(),
            moved_to.display()
        ),
        Err(e) => {
            return (
                empty(true),
                Some(format!(
                    "{} is damaged ({reason}) and could not be moved aside: {e}",
                    path.display()
                )),
            );
        }
    };
    (empty(false), Some(problem))
}

/// Renames a damaged file to `<name>.broken-<unix time>`.
fn set_aside(path: &Path) -> io::Result<PathBuf> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut name = path.file_name().expect("a file").to_owned();
    name.push(format!(".broken-{seconds}"));
    let moved_to = path.with_file_name(name);
    fs::rename(path, &moved_to)?;
    Ok(moved_to)
}

/// Writes the file atomically, readable only by the user.
fn save(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().expect("a file in a directory");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(path.file_name().expect("a file"));
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = dir.join(temp_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn object(value: Value) -> Settings {
        match value {
            Value::Object(map) => map,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = GlobalSettings::new(temp.path());
        assert_eq!(store.get("com.example.a"), (Settings::new(), None));
        store
            .set(
                "com.example.a",
                object(json!({ "token": "secret", "n": 1 })),
            )
            .unwrap();

        let path = temp.path().join("com.example.a/settings.json");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"formatVersion\": 1,\n  \"settings\": {\n    \"token\": \"secret\",\n    \"n\": 1\n  }\n}\n"
        );
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let mut again = GlobalSettings::new(temp.path());
        assert_eq!(
            again.get("com.example.a").0,
            object(json!({ "token": "secret", "n": 1 }))
        );
    }

    #[test]
    fn damaged_files_are_moved_aside() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("com.example.a");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("settings.json"), "{ not json").unwrap();
        let mut store = GlobalSettings::new(temp.path());
        let (settings, problem) = store.get("com.example.a");
        assert!(settings.is_empty());
        assert!(problem.unwrap().contains("is damaged"));
        assert!(!dir.join("settings.json").exists());
        let moved: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(moved.len(), 1);
        store.set("com.example.a", Settings::new()).unwrap();
    }

    #[test]
    fn newer_files_are_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("com.example.a");
        fs::create_dir(&dir).unwrap();
        let newer = r#"{ "formatVersion": 2, "settings": {}, "more": true }"#;
        fs::write(dir.join("settings.json"), newer).unwrap();
        let mut store = GlobalSettings::new(temp.path());
        assert!(store.get("com.example.a").1.unwrap().contains("newer"));
        assert!(store.set("com.example.a", Settings::new()).is_err());
        assert_eq!(
            fs::read_to_string(dir.join("settings.json")).unwrap(),
            newer
        );
    }
}
