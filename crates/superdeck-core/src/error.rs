// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

/// Errors from reading and writing SuperDeck's files.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file couldn't be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A newer SuperDeck wrote the file. It's left untouched.
    #[error(
        "{} was written by a newer SuperDeck (format version {found}; this version reads up to {supported})",
        path.display()
    )]
    TooNew {
        /// The file.
        path: PathBuf,
        /// Its format version.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },

    /// The file isn't valid, so it was moved aside rather than ever being
    /// overwritten.
    #[error("{} is damaged ({reason}); it was moved to {}", path.display(), moved_to.display())]
    Damaged {
        /// Where the file was.
        path: PathBuf,
        /// Where it is now.
        moved_to: PathBuf,
        /// What's wrong with it.
        reason: String,
    },

    /// The data to save breaks a rule of the format, so nothing was written.
    #[error("not saving {}: {reason}", path.display())]
    Invalid {
        /// The file that would have been written.
        path: PathBuf,
        /// The rule broken.
        reason: String,
    },

    /// An image isn't PNG, JPEG, GIF, or SVG.
    #[error("unsupported image format; use PNG, JPEG, GIF, or SVG")]
    UnsupportedImage,
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Error::Io { path, source }
    }

    /// Whether this is a file that doesn't exist.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }
}
