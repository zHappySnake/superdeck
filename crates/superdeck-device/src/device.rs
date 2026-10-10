// SPDX-License-Identifier: GPL-3.0-or-later

//! An open device: key images, brightness, input.

use std::fmt;
use std::time::{Duration, Instant};

use image::{DynamicImage, Rgb, RgbImage};

use crate::protocol::{FeatureQuery, extract_string};
use crate::{Error, Model, Transport, key_image};

/// A key was pressed or released. Keys are numbered row by row from the
/// top left, starting at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEvent {
    /// The key went down.
    Down(u8),
    /// The key came back up.
    Up(u8),
}

impl KeyEvent {
    /// The key this event is about.
    #[must_use]
    pub fn key(self) -> u8 {
        match self {
            KeyEvent::Down(key) | KeyEvent::Up(key) => key,
        }
    }
}

/// An open Stream Deck.
///
/// All calls block until the device has the data. A `Device` isn't shared
/// between threads; the owner runs input reads and output commands on the
/// same thread, or moves the device to a dedicated thread.
pub struct Device {
    model: Model,
    transport: Box<dyn Transport>,
    /// Last known state of each key, in SuperDeck's key order.
    pressed: Vec<bool>,
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device")
            .field("model", &self.model)
            .field("pressed", &self.pressed)
            .finish_non_exhaustive()
    }
}

impl Device {
    /// Drives `model` over `transport`. Use
    /// [`DeviceManager::open`](crate::DeviceManager::open) for real devices.
    ///
    /// Aborts any image transfer a previous program left unfinished.
    pub fn with_transport(model: Model, mut transport: Box<dyn Transport>) -> Result<Self, Error> {
        transport.write(&model.spec().protocol.reset_key_stream_report())?;
        Ok(Self {
            model,
            transport,
            pressed: vec![false; usize::from(model.key_count())],
        })
    }

    /// The device's model.
    #[must_use]
    pub fn model(&self) -> Model {
        self.model
    }

    /// Number of keys.
    #[must_use]
    pub fn key_count(&self) -> u8 {
        self.model.key_count()
    }

    /// Serial number, read from the device.
    pub fn serial_number(&mut self) -> Result<String, Error> {
        let query = self.model.spec().protocol.serial_number_query();
        self.query_string(query)
    }

    /// Firmware version, read from the device.
    pub fn firmware_version(&mut self) -> Result<String, Error> {
        let query = self.model.spec().protocol.firmware_version_query();
        self.query_string(query)
    }

    fn query_string(&mut self, query: FeatureQuery) -> Result<String, Error> {
        let report = self
            .transport
            .get_feature_report(query.report_id, query.len)?;
        if report.len() <= query.offset {
            return Err(Error::ShortTransfer {
                expected: query.len,
                actual: report.len(),
            });
        }
        Ok(extract_string(&report, query.offset))
    }

    /// Sets the backlight brightness, 0 to 100 percent. Larger values are
    /// treated as 100.
    pub fn set_brightness(&mut self, percent: u8) -> Result<(), Error> {
        let report = self.model.spec().protocol.brightness_report(percent);
        self.transport.send_feature_report(&report)
    }

    /// Resets the device: it shows its boot logo and forgets all key images.
    pub fn reset(&mut self) -> Result<(), Error> {
        let report = self.model.spec().protocol.reset_report();
        self.transport.send_feature_report(&report)
    }

    /// Shows `image` on `key`, scaled to fit and centered on black.
    pub fn set_key_image(&mut self, key: u8, image: &DynamicImage) -> Result<(), Error> {
        self.check_key(key)?;
        let data = key_image::encode_key_image(self.model.spec(), image)?;
        self.set_key_image_data(key, &data)
    }

    /// Shows an image already in the device's native format (see
    /// [`encode_key_image`](crate::encode_key_image)), skipping the encoding
    /// step. Useful for caching encoded images.
    pub fn set_key_image_data(&mut self, key: u8, data: &[u8]) -> Result<(), Error> {
        self.check_key(key)?;
        let spec = self.model.spec();
        let max = spec.protocol.max_image_len();
        if data.len() > max {
            return Err(Error::ImageTooLarge {
                len: data.len(),
                max,
            });
        }
        let device_key = spec.protocol.device_key(key, spec.columns);
        for report in spec.protocol.image_reports(device_key, data) {
            self.transport.write(&report)?;
        }
        Ok(())
    }

    /// Fills `key` with a solid color.
    pub fn set_key_color(&mut self, key: u8, color: Rgb<u8>) -> Result<(), Error> {
        let size = self.model.spec().key_size;
        let image = RgbImage::from_pixel(size, size, color);
        self.set_key_image(key, &DynamicImage::ImageRgb8(image))
    }

    /// Turns every key black.
    pub fn clear(&mut self) -> Result<(), Error> {
        let size = self.model.spec().key_size;
        let black = DynamicImage::ImageRgb8(RgbImage::new(size, size));
        let data = key_image::encode_key_image(self.model.spec(), &black)?;
        for key in 0..self.key_count() {
            self.set_key_image_data(key, &data)?;
        }
        Ok(())
    }

    /// Whether each key is currently held down, as of the last read.
    #[must_use]
    pub fn pressed_keys(&self) -> &[bool] {
        &self.pressed
    }

    /// Waits up to `timeout` (forever if `None`) for input and returns the
    /// key presses and releases it contains. An empty result means nothing
    /// changed before the timeout.
    pub fn read_key_events(&mut self, timeout: Option<Duration>) -> Result<Vec<KeyEvent>, Error> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let spec = self.model.spec();
        let len = spec
            .protocol
            .input_report_len(spec.key_count(), spec.extra_inputs);
        loop {
            let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            let Some(report) = self.transport.read(len, remaining)? else {
                return Ok(Vec::new());
            };
            let events = self.update_key_states(&report);
            if !events.is_empty() {
                return Ok(events);
            }
            if remaining.is_some_and(|r| r.is_zero()) {
                return Ok(Vec::new());
            }
        }
    }

    /// Applies a key state report and returns what changed.
    fn update_key_states(&mut self, report: &[u8]) -> Vec<KeyEvent> {
        let spec = self.model.spec();
        let Some(device_states) = spec.protocol.parse_key_states(report, spec.key_count()) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for key in 0..spec.key_count() {
            let device_key = spec.protocol.device_key(key, spec.columns);
            let now = device_states[usize::from(device_key)];
            let before = &mut self.pressed[usize::from(key)];
            if now != *before {
                *before = now;
                events.push(if now {
                    KeyEvent::Down(key)
                } else {
                    KeyEvent::Up(key)
                });
            }
        }
        events
    }

    fn check_key(&self, key: u8) -> Result<(), Error> {
        if key < self.key_count() {
            Ok(())
        } else {
            Err(Error::KeyOutOfRange {
                key,
                key_count: self.key_count(),
            })
        }
    }
}
