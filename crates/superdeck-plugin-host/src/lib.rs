// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs Stream Deck plugins for SuperDeck, as described in
//! `sdk/protocol/README.md`.
//!
//! [`discover`] finds the plugins in the plugin directories and reads their
//! manifests.
//!
//! ```no_run
//! use superdeck_plugin_host::{Entry, discover, plugin_dirs};
//!
//! let found = discover(&plugin_dirs());
//! for plugin in found.plugins.values() {
//!     match &plugin.entry {
//!         Entry::Node { .. } => println!("{}: runs on Node.js", plugin.uuid),
//!         Entry::Unsupported(why) => println!("{}: can't run, {why}", plugin.uuid),
//!     }
//! }
//! for problem in &found.problems {
//!     eprintln!("{problem}");
//! }
//! ```

mod discovery;
mod error;
pub mod manifest;

pub use discovery::{Discovery, discover, plugin_dirs};
pub use error::Error;
pub use manifest::{Entry, Manifest, Plugin};
