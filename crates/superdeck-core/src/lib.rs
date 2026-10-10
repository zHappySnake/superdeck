// SPDX-License-Identifier: GPL-3.0-or-later

//! SuperDeck's application model: profiles, pages, folders, and actions,
//! and the files they're kept in.
//!
//! The types in [`model`] and [`devices`] mirror the files described in
//! `docs/profile-format.md`. [`ConfigDir`] reads and writes them: migrating
//! old versions, refusing newer ones, checking the format's rules, and
//! replacing files atomically.
//!
//! ```no_run
//! use superdeck_core::{ConfigDir, Layout, Profile};
//!
//! # fn main() -> Result<(), superdeck_core::Error> {
//! let config = ConfigDir::from_env().expect("no home directory");
//! let profile = Profile::new("Default", Layout { columns: 5, rows: 3 });
//! config.save_profile(&profile)?;
//! let loaded = config.load_profile(profile.id)?;
//! assert_eq!(loaded.value, profile);
//! # Ok(())
//! # }
//! ```

pub mod devices;
mod error;
mod format;
mod ids;
pub mod model;
mod store;
pub mod validate;

pub use devices::{DeviceSettings, Devices};
pub use error::Error;
pub use format::{DEVICES_VERSION, PROFILE_VERSION};
pub use ids::{InstanceId, PageId, ProfileId};
pub use model::{
    Action, ActionInstance, Appearance, CoreAction, Keys, Layout, Page, PluginAction, Position,
    Profile,
};
pub use store::{ConfigDir, Loaded};
