// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs Stream Deck plugins for SuperDeck, as described in
//! `sdk/protocol/README.md`.
//!
//! [`discover`] finds the plugins in the plugin directories and reads their
//! manifests. [`PluginHost`] runs them: a WebSocket server on `127.0.0.1`,
//! a supervised process per Node.js plugin, and routing between plugins,
//! property inspectors, and the app. The app sends [`message::Event`]s
//! about its keys and devices, and carries out the [`Request`]s that come
//! back, such as setting a title or saving settings.
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
mod global_settings;
mod host;
mod images;
mod info;
mod log;
pub mod manifest;
pub mod message;
mod node;
mod process;
mod router;
mod server;

pub use discovery::{Discovery, discover, plugin_dirs};
pub use error::Error;
pub use global_settings::GLOBAL_SETTINGS_VERSION;
pub use host::{Config, Launch, PluginHost};
pub use info::{AppInfo, COMPATIBLE_VERSION};
pub use manifest::{Entry, Manifest, Plugin};
pub use node::{MIN_NODE_MAJOR, NodeRuntime};
pub use router::{InspectorLaunch, PluginLaunch, Request, Sender};
