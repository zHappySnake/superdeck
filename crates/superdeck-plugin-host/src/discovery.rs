// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding plugins: the `<uuid>.sdPlugin` directories in the plugin
//! directories.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::manifest::{PLUGIN_DIR_SUFFIX, Plugin};

/// The plugins found, and the directories that couldn't be read as one.
#[derive(Debug, Default)]
pub struct Discovery {
    /// Plugins by UUID.
    pub plugins: BTreeMap<String, Plugin>,
    /// Plugin directories that couldn't be read, and why. Each disables
    /// only that plugin.
    pub problems: Vec<Error>,
}

/// The standard plugin directories, in the order [`discover`] wants them:
/// the bundled ones from `$XDG_DATA_DIRS`, then the user's in
/// `$XDG_DATA_HOME`.
#[must_use]
pub fn plugin_dirs() -> Vec<PathBuf> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    // Earlier entries of $XDG_DATA_DIRS are more important, so they come
    // later here.
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&data_dirs)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("superdeck/plugins"))
        .collect();
    dirs.reverse();
    if let Some(data_home) =
        absolute("XDG_DATA_HOME").or_else(|| Some(absolute("HOME")?.join(".local/share")))
    {
        dirs.push(data_home.join("superdeck/plugins"));
    }
    dirs
}

/// Reads every plugin in `dirs`. A plugin in a later directory replaces
/// one with the same UUID in an earlier one, so user-installed plugins go
/// last. Directories that don't exist are skipped.
#[must_use]
pub fn discover(dirs: &[PathBuf]) -> Discovery {
    let mut discovery = Discovery::default();
    for dir in dirs {
        let mut entries = match plugin_dirs_in(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                discovery.problems.push(Error::io(dir)(e));
                continue;
            }
        };
        entries.sort();
        for entry in entries {
            match Plugin::load(&entry) {
                Ok(plugin) => {
                    discovery.plugins.insert(plugin.uuid.clone(), plugin);
                }
                Err(e) => discovery.problems.push(e),
            }
        }
    }
    discovery
}

fn plugin_dirs_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let is_plugin = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(PLUGIN_DIR_SUFFIX));
        if is_plugin && path.is_dir() {
            found.push(path);
        }
    }
    Ok(found)
}
