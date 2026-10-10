// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

/// Errors from reading plugins and running them.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file or directory couldn't be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A file isn't valid JSON, or doesn't have the expected shape.
    #[error("{}: {source}", path.display())]
    Json {
        /// The file.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// A manifest breaks a rule SuperDeck relies on.
    #[error("{}: {reason}", path.display())]
    InvalidManifest {
        /// The manifest.
        path: PathBuf,
        /// The rule broken.
        reason: String,
    },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Error::Io { path, source }
    }
}
