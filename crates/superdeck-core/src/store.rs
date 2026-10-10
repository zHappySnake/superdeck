// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading and writing the configuration directory: `devices.json` and one
//! directory per profile.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::devices::Devices;
use crate::format::{self, Format, Read};
use crate::ids::ProfileId;
use crate::model::{ImagePath, Profile};
use crate::{Error, validate};

const DEVICES_FILE: &str = "devices.json";
const PROFILES_DIR: &str = "profiles";
const PROFILE_FILE: &str = "profile.json";

/// A file as read, and whether it was migrated from an older version.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded<T> {
    /// The contents, in the current version.
    pub value: T,
    /// The version it was read as, if older than current. Saving it writes
    /// the current version, after keeping a copy of the old file.
    pub migrated_from: Option<u32>,
}

/// SuperDeck's configuration directory, usually `~/.config/superdeck`.
///
/// Every write replaces files atomically. A file that can't be read is moved
/// aside, never overwritten, and a file from a newer version is left alone.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    root: PathBuf,
}

impl ConfigDir {
    /// The configuration directory at `root`. Nothing is created until
    /// something is saved.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$XDG_CONFIG_HOME/superdeck`, or `~/.config/superdeck`. `None` if
    /// neither variable is set to an absolute path.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let absolute = |name| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let config =
            absolute("XDG_CONFIG_HOME").or_else(|| Some(absolute("HOME")?.join(".config")))?;
        Some(Self::new(config.join("superdeck")))
    }

    /// The directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn devices_file(&self) -> PathBuf {
        self.root.join(DEVICES_FILE)
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join(PROFILES_DIR)
    }

    /// The directory of a profile.
    #[must_use]
    pub fn profile_dir(&self, id: ProfileId) -> PathBuf {
        self.profiles_dir().join(id.to_string())
    }

    /// The file an image of a profile is stored in.
    #[must_use]
    pub fn image_file(&self, profile: ProfileId, image: &ImagePath) -> PathBuf {
        self.profile_dir(profile).join(image.as_str())
    }

    /// Reads `devices.json`. A missing file reads as no devices.
    pub fn load_devices(&self) -> Result<Loaded<Devices>, Error> {
        let path = self.devices_file();
        match fs::read_to_string(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Loaded {
                value: Devices::default(),
                migrated_from: None,
            }),
            Err(e) => Err(Error::io(&path)(e)),
            Ok(text) => load(&format::DEVICES, &path, &text, validate::devices),
        }
    }

    /// Writes `devices.json`.
    pub fn save_devices(&self, devices: &Devices) -> Result<(), Error> {
        let path = self.devices_file();
        validate::devices(devices).map_err(|reason| Error::Invalid {
            path: path.clone(),
            reason,
        })?;
        save(&format::DEVICES, &path, devices)
    }

    /// IDs of every profile directory, sorted. Directories whose names
    /// aren't profile IDs are ignored.
    pub fn profile_ids(&self) -> Result<Vec<ProfileId>, Error> {
        let dir = self.profiles_dir();
        let entries = match fs::read_dir(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            entries => entries.map_err(Error::io(&dir))?,
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(Error::io(&dir))?;
            let id = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok());
            if let Some(id) = id
                && entry.file_type().map_err(Error::io(entry.path()))?.is_dir()
            {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Reads a profile.
    pub fn load_profile(&self, id: ProfileId) -> Result<Loaded<Profile>, Error> {
        let path = self.profile_dir(id).join(PROFILE_FILE);
        let text = fs::read_to_string(&path).map_err(Error::io(&path))?;
        load(&format::PROFILE, &path, &text, |profile: &Profile| {
            if profile.id != id {
                return Err(format!("its ID {} doesn't match its directory", profile.id));
            }
            validate::profile(profile)
        })
    }

    /// Writes a profile, then deletes images no key uses any more.
    pub fn save_profile(&self, profile: &Profile) -> Result<(), Error> {
        let dir = self.profile_dir(profile.id);
        let path = dir.join(PROFILE_FILE);
        validate::profile(profile).map_err(|reason| Error::Invalid {
            path: path.clone(),
            reason,
        })?;
        fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
        save(&format::PROFILE, &path, profile)?;
        self.delete_unused_images(profile)
    }

    /// Deletes a profile and its images.
    pub fn delete_profile(&self, id: ProfileId) -> Result<(), Error> {
        let dir = self.profile_dir(id);
        match fs::remove_dir_all(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result.map_err(Error::io(&dir)),
        }
    }

    /// Stores an image for a profile and returns the path to put in an
    /// appearance. Storing the same image again returns the same path.
    pub fn add_image(&self, profile: ProfileId, data: &[u8]) -> Result<ImagePath, Error> {
        let extension = image_extension(data).ok_or(Error::UnsupportedImage)?;
        let hash = Sha256::digest(data)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        let image = ImagePath::from_parts(&hash, extension);
        let path = self.image_file(profile, &image);
        if !path.exists() {
            let dir = self.profile_dir(profile).join(ImagePath::DIR);
            fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
            write_atomic(&path, data).map_err(Error::io(&path))?;
        }
        Ok(image)
    }

    fn delete_unused_images(&self, profile: &Profile) -> Result<(), Error> {
        let used: HashSet<&str> = profile
            .instances()
            .flat_map(|instance| &instance.appearance)
            .filter_map(|appearance| appearance.image.as_ref())
            .map(ImagePath::file_name)
            .collect();
        let dir = self.profile_dir(profile.id).join(ImagePath::DIR);
        let entries = match fs::read_dir(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            entries => entries.map_err(Error::io(&dir))?,
        };
        for entry in entries {
            let entry = entry.map_err(Error::io(&dir))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            // Only files we could have written; leave anything else alone.
            let ours = format!("{}/{name}", ImagePath::DIR)
                .parse::<ImagePath>()
                .is_ok();
            if ours && !used.contains(name) {
                fs::remove_file(entry.path()).map_err(Error::io(entry.path()))?;
            }
        }
        Ok(())
    }
}

/// Reads and checks a versioned file, moving it aside if it's damaged.
fn load<T: DeserializeOwned>(
    format: &Format,
    path: &Path,
    text: &str,
    check: impl FnOnce(&T) -> Result<(), String>,
) -> Result<Loaded<T>, Error> {
    let reason = match format.read::<T>(text) {
        Read::TooNew(found) => {
            return Err(Error::TooNew {
                path: path.to_owned(),
                found,
                supported: format.version,
            });
        }
        Read::Ok { value, from } => match check(&value) {
            Ok(()) => {
                return Ok(Loaded {
                    value,
                    migrated_from: (from < format.version).then_some(from),
                });
            }
            Err(reason) => reason,
        },
        Read::Invalid(reason) => reason,
    };
    let moved_to = set_aside(path)?;
    Err(Error::Damaged {
        path: path.to_owned(),
        moved_to,
        reason,
    })
}

/// Renames a damaged file to `<name>.broken-<unix time>`.
fn set_aside(path: &Path) -> Result<PathBuf, Error> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut name = path.file_name().expect("a file").to_owned();
    name.push(format!(".broken-{seconds}"));
    let moved_to = path.with_file_name(name);
    fs::rename(path, &moved_to).map_err(Error::io(path))?;
    Ok(moved_to)
}

/// Writes a versioned file. Keeps a copy of an older version's file as
/// `<name>.v<version>.bak`, and refuses to replace a file it can't read or
/// one from a newer version.
fn save<T: Serialize>(format: &Format, path: &Path, value: &T) -> Result<(), Error> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(path)(e)),
        Ok(existing) => match Format::version_of(&existing) {
            Some(version) if version > format.version => {
                return Err(Error::TooNew {
                    path: path.to_owned(),
                    found: version,
                    supported: format.version,
                });
            }
            Some(version) if version < format.version => {
                let mut name = path.file_name().expect("a file").to_owned();
                name.push(format!(".v{version}.bak"));
                let backup = path.with_file_name(name);
                if !backup.exists() {
                    fs::copy(path, &backup).map_err(Error::io(&backup))?;
                }
            }
            Some(_) => {}
            None => {
                return Err(Error::Invalid {
                    path: path.to_owned(),
                    reason: "the file there can't be read, so it isn't replaced".to_owned(),
                });
            }
        },
    }
    let text = format.write(value).map_err(|e| Error::Invalid {
        path: path.to_owned(),
        reason: e.to_string(),
    })?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(Error::io(dir))?;
    }
    write_atomic(path, text.as_bytes()).map_err(Error::io(path))
}

/// Writes a file so that a crash leaves either the old or the new one:
/// write a temporary file, flush it to disk, rename it over the old one,
/// then flush the directory.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().expect("a file in a directory");
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(path.file_name().expect("a file"));
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = dir.join(temp_name);
    let result = (|| {
        let mut file = File::create(&temp)?;
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

/// The extension for an image's format, recognized from its content.
fn image_extension(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        let text = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
        text.trim_ascii_start().starts_with(b"<").then_some("svg")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_formats() {
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_extension(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(image_extension(b"GIF89a..."), Some("gif"));
        assert_eq!(image_extension(b"\xef\xbb\xbf  <svg/>"), Some("svg"));
        assert_eq!(image_extension(b"BM...."), None);
        assert_eq!(image_extension(b""), None);
    }
}
