// SPDX-License-Identifier: GPL-3.0-or-later

//! Format versions and migrations: reading a file of any supported version
//! into the current types, and writing the current version.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Current format version of `profile.json`.
pub const PROFILE_VERSION: u32 = 1;

/// Current format version of `devices.json`.
pub const DEVICES_VERSION: u32 = 1;

/// Turns a file of one version into the next, in place. Receives the file's
/// JSON object without `formatVersion`.
type Migration = fn(&mut Map<String, Value>) -> Result<(), String>;

/// A versioned file format.
pub(crate) struct Format {
    pub version: u32,
    /// `migrations[i]` turns version `i + 1` into version `i + 2`.
    migrations: &'static [Migration],
}

pub(crate) const PROFILE: Format = Format {
    version: PROFILE_VERSION,
    migrations: &PROFILE_MIGRATIONS,
};

pub(crate) const DEVICES: Format = Format {
    version: DEVICES_VERSION,
    migrations: &DEVICES_MIGRATIONS,
};

// One migration per version step; the array lengths keep them in sync with
// the version numbers.
const PROFILE_MIGRATIONS: [Migration; PROFILE_VERSION as usize - 1] = [];
const DEVICES_MIGRATIONS: [Migration; DEVICES_VERSION as usize - 1] = [];

/// The result of reading a file.
#[derive(Debug)]
pub(crate) enum Read<T> {
    /// Read, after migrating from `from` if it's older than current.
    Ok { value: T, from: u32 },
    /// Written by a newer version.
    TooNew(u32),
    /// Not valid JSON, or not a valid file of its version.
    Invalid(String),
}

impl Format {
    /// Parses a file, migrating it to the current version.
    pub fn read<T: DeserializeOwned>(&self, text: &str) -> Read<T> {
        let mut map = match serde_json::from_str(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Read::Invalid("not a JSON object".to_owned()),
            Err(e) => return Read::Invalid(e.to_string()),
        };
        let from = match map.remove("formatVersion").as_ref().and_then(Value::as_u64) {
            Some(version) if version >= 1 => u32::try_from(version).unwrap_or(u32::MAX),
            _ => return Read::Invalid("no valid formatVersion".to_owned()),
        };
        if from > self.version {
            return Read::TooNew(from);
        }
        for (step, migrate) in (from..).zip(&self.migrations[from as usize - 1..]) {
            if let Err(e) = migrate(&mut map) {
                return Read::Invalid(format!("migrating from version {step}: {e}"));
            }
        }
        match T::deserialize(Value::Object(map)) {
            Ok(value) => Read::Ok { value, from },
            Err(e) => Read::Invalid(e.to_string()),
        }
    }

    /// The format version a file declares, without reading the rest.
    pub fn version_of(text: &str) -> Option<u32> {
        #[derive(Deserialize)]
        struct Header {
            #[serde(rename = "formatVersion")]
            format_version: u32,
        }
        serde_json::from_str::<Header>(text)
            .ok()
            .map(|header| header.format_version)
    }

    /// Writes `value` as the current version: pretty-printed, with
    /// `formatVersion` first and a trailing newline.
    pub fn write<T: Serialize>(&self, value: &T) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Versioned<'a, T> {
            #[serde(rename = "formatVersion")]
            format_version: u32,
            #[serde(flatten)]
            value: &'a T,
        }
        let mut text = serde_json::to_string_pretty(&Versioned {
            format_version: self.version,
            value,
        })?;
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Thing {
        name: String,
    }

    fn rename(map: &mut Map<String, Value>) -> Result<(), String> {
        let old = map.remove("title").ok_or("no title")?;
        map.insert("name".to_owned(), old);
        Ok(())
    }

    const V2: Format = Format {
        version: 2,
        migrations: &[rename],
    };

    #[test]
    fn writes_the_version_first() {
        let text = V2
            .write(&Thing {
                name: "a".to_owned(),
            })
            .unwrap();
        assert_eq!(text, "{\n  \"formatVersion\": 2,\n  \"name\": \"a\"\n}\n");
        assert_eq!(Format::version_of(&text), Some(2));
    }

    #[test]
    fn reads_current_and_migrates_old() {
        let current = json!({ "formatVersion": 2, "name": "a" }).to_string();
        assert!(matches!(
            V2.read::<Thing>(&current),
            Read::Ok { from: 2, .. }
        ));
        let old = json!({ "formatVersion": 1, "title": "a" }).to_string();
        let Read::Ok { value, from } = V2.read::<Thing>(&old) else {
            panic!("expected a migrated read");
        };
        assert_eq!((value.name.as_str(), from), ("a", 1));
    }

    #[test]
    fn refuses_newer_and_invalid() {
        let newer = json!({ "formatVersion": 3, "whatever": true }).to_string();
        assert!(matches!(V2.read::<Thing>(&newer), Read::TooNew(3)));
        for bad in [
            "not json",
            "[1]",
            "{\"name\": \"a\"}",
            "{\"formatVersion\": 0, \"name\": \"a\"}",
            "{\"formatVersion\": \"2\", \"name\": \"a\"}",
            "{\"formatVersion\": 2, \"name\": \"a\", \"extra\": 1}",
            "{\"formatVersion\": 1}",
        ] {
            assert!(matches!(V2.read::<Thing>(bad), Read::Invalid(_)), "{bad}");
        }
    }
}
