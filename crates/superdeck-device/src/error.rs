// SPDX-License-Identifier: GPL-3.0-or-later

/// Errors from the device layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A HID call failed. Once a device is unplugged, every call on it fails
    /// this way.
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),

    /// The device couldn't be opened, usually for lack of permissions.
    #[error(
        "could not open {path}: {source} (is the udev rule installed? See packaging/udev/70-superdeck.rules)"
    )]
    Open {
        /// Device path.
        path: String,
        /// Underlying error.
        #[source]
        source: hidapi::HidError,
    },

    /// No connected device has that path.
    #[error("no supported device at {0}")]
    NotFound(String),

    /// An I/O error, from udev or a capture file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The device sent or accepted fewer bytes than expected.
    #[error("short transfer: expected {expected} bytes, got {actual}")]
    ShortTransfer {
        /// Bytes expected.
        expected: usize,
        /// Bytes transferred.
        actual: usize,
    },

    /// The key index doesn't exist on this device.
    #[error("key {key} doesn't exist; this device has keys 0 to {}", key_count.saturating_sub(1))]
    KeyOutOfRange {
        /// Requested key.
        key: u8,
        /// Keys on the device.
        key_count: u8,
    },

    /// An encoded image is too large to send.
    #[error("encoded image is {len} bytes; the device accepts at most {max}")]
    ImageTooLarge {
        /// Size of the image.
        len: usize,
        /// Largest size the protocol can carry.
        max: usize,
    },

    /// An image couldn't be encoded.
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    /// A capture file is malformed, or a replay didn't match it.
    #[error("capture: {0}")]
    Capture(String),
}
