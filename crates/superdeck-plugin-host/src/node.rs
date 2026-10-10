// SPDX-License-Identifier: GPL-3.0-or-later

//! The Node.js runtime every JavaScript plugin runs on.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// Oldest major version of Node.js plugins run on.
pub const MIN_NODE_MAJOR: u32 = 24;

/// A Node.js runtime, checked to be new enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRuntime {
    /// The `node` executable.
    pub path: PathBuf,
    /// Its version, such as `v24.1.0`.
    pub version: String,
}

impl NodeRuntime {
    /// The runtime in `$SUPERDECK_NODE`, which packages that bundle Node.js
    /// set, or else `node` on `$PATH`.
    pub fn find() -> Result<Self, Error> {
        if let Some(path) = std::env::var_os("SUPERDECK_NODE").filter(|path| !path.is_empty()) {
            return Self::at(Path::new(&path));
        }
        let path = std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|dir| dir.join("node"))
            .find(|candidate| is_executable(candidate))
            .ok_or(Error::NodeNotFound)?;
        Self::at(&path)
    }

    /// The runtime at `path`.
    pub fn at(path: &Path) -> Result<Self, Error> {
        let output = Command::new(path)
            .arg("--version")
            .output()
            .map_err(Error::io(path))?;
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let major = version
            .strip_prefix('v')
            .and_then(|rest| rest.split('.').next())
            .and_then(|major| major.parse::<u32>().ok());
        match major {
            Some(major) if output.status.success() && major >= MIN_NODE_MAJOR => Ok(Self {
                path: path.to_owned(),
                version,
            }),
            _ => Err(Error::NodeTooOld {
                path: path.to_owned(),
                version,
            }),
        }
    }
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fake_node(dir: &Path, version: &str) -> PathBuf {
        let path = dir.join(format!("node-{version}"));
        fs::write(&path, format!("#!/bin/sh\necho {version}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn versions() {
        let temp = tempfile::tempdir().unwrap();
        let node = NodeRuntime::at(&fake_node(temp.path(), "v24.3.1")).unwrap();
        assert_eq!(node.version, "v24.3.1");
        assert!(NodeRuntime::at(&fake_node(temp.path(), "v26.0.0")).is_ok());
        for old in ["v22.11.0", "garbage"] {
            assert!(matches!(
                NodeRuntime::at(&fake_node(temp.path(), old)),
                Err(Error::NodeTooOld { version, .. }) if version == old
            ));
        }
        assert!(matches!(
            NodeRuntime::at(&temp.path().join("missing")),
            Err(Error::Io { .. })
        ));
    }
}
