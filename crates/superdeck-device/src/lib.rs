// SPDX-License-Identifier: GPL-3.0-or-later

//! Drivers for Elgato Stream Deck devices over USB HID.
//!
//! [`DeviceManager`] lists connected devices, opens them, and reports when
//! they're plugged in or unplugged. An open [`Device`] shows key images, sets
//! the brightness, and reads key presses. It knows nothing about profiles or
//! plugins.
//!
//! ```no_run
//! use superdeck_device::{DeviceManager, KeyEvent};
//! use superdeck_device::image::Rgb;
//!
//! # fn main() -> Result<(), superdeck_device::Error> {
//! let mut manager = DeviceManager::new()?;
//! let info = manager.devices()?.into_iter().next().expect("no device");
//! let mut deck = manager.open(&info)?;
//! deck.set_brightness(60)?;
//! deck.set_key_color(0, Rgb([255, 0, 0]))?;
//! loop {
//!     for event in deck.read_key_events(None)? {
//!         if let KeyEvent::Down(key) = event {
//!             println!("key {key} pressed");
//!         }
//!     }
//! }
//! # }
//! ```
//!
//! The protocol logic is pure ([`Transport`] is the only I/O boundary), so it
//! is tested by replaying [captures](capture) of USB traffic.

pub mod capture;
mod device;
mod discovery;
mod error;
mod hotplug;
mod key_image;
mod model;
mod protocol;
mod transport;

pub use capture::{Capture, Recorder, Replay};
pub use device::{Device, KeyEvent};
pub use discovery::{DeviceChange, DeviceInfo, DeviceManager};
pub use error::Error;
pub use key_image::{ImageFormat, Rotation, Transform, encode_key_image, fit_to_key};
pub use model::{ELGATO_VENDOR_ID, Model, Spec};
pub use protocol::Protocol;
pub use transport::{HidTransport, Transport};

/// The `image` crate, re-exported so callers use the same version.
pub use ::image;
