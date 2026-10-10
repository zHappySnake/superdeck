// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding devices, opening them, and noticing when they come and go.

use std::ffi::{CStr, CString};
use std::fmt;
use std::thread;
use std::time::{Duration, Instant};

use hidapi::HidApi;

use crate::hotplug::HotplugMonitor;
use crate::transport::HidTransport;
use crate::{Device, Error, Model, Transport};

/// How often to rescan when udev events aren't available.
const RESCAN_INTERVAL: Duration = Duration::from_secs(1);

/// A connected, supported device that hasn't necessarily been opened.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    /// The model.
    pub model: Model,
    /// Serial number from the USB descriptor, if the device reports one.
    pub serial_number: Option<String>,
    /// Platform path used to open the device (`/dev/hidrawN`).
    pub path: CString,
}

impl DeviceInfo {
    /// The path as text, for display and for matching hotplug events.
    #[must_use]
    pub fn path_lossy(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.model)?;
        if let Some(serial) = &self.serial_number {
            write!(f, " ({serial})")?;
        }
        Ok(())
    }
}

/// A device appeared or disappeared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChange {
    /// A supported device was plugged in.
    Connected(DeviceInfo),
    /// A device was unplugged. Any [`Device`] opened from it now fails.
    Disconnected(DeviceInfo),
}

/// Lists, opens, and watches devices.
pub struct DeviceManager {
    api: HidApi,
    /// `None` if udev events aren't available; then we rescan periodically.
    monitor: Option<HotplugMonitor>,
    /// Devices seen by the last scan, sorted by path.
    known: Vec<DeviceInfo>,
}

impl fmt::Debug for DeviceManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceManager")
            .field("hotplug_events", &self.monitor.is_some())
            .field("known", &self.known)
            .finish_non_exhaustive()
    }
}

impl DeviceManager {
    /// Starts watching for hotplug events and scans for devices.
    pub fn new() -> Result<Self, Error> {
        // Subscribe before the first scan so nothing plugged in between the
        // two is missed.
        let monitor = HotplugMonitor::new().ok();
        let api = HidApi::new()?;
        let mut manager = Self {
            api,
            monitor,
            known: Vec::new(),
        };
        manager.known = manager.scan()?;
        Ok(manager)
    }

    /// Whether hotplug uses udev events. If not, [`DeviceManager::wait_for_changes`]
    /// falls back to rescanning every second.
    #[must_use]
    pub fn has_hotplug_events(&self) -> bool {
        self.monitor.is_some()
    }

    /// Rescans and returns every connected supported device, sorted by path.
    ///
    /// This also resets the baseline for [`DeviceManager::wait_for_changes`].
    pub fn devices(&mut self) -> Result<Vec<DeviceInfo>, Error> {
        self.known = self.scan()?;
        Ok(self.known.clone())
    }

    fn scan(&mut self) -> Result<Vec<DeviceInfo>, Error> {
        self.api.refresh_devices()?;
        let mut devices: Vec<DeviceInfo> = self
            .api
            .device_list()
            .filter_map(|info| {
                let model = Model::from_usb_id(info.vendor_id(), info.product_id())?;
                Some(DeviceInfo {
                    model,
                    serial_number: info
                        .serial_number()
                        .map(str::to_owned)
                        .filter(|s| !s.is_empty()),
                    path: info.path().to_owned(),
                })
            })
            .collect();
        devices.sort_by(|a, b| a.path.cmp(&b.path));
        // A device can show up once per HID interface.
        devices.dedup_by(|a, b| a.path == b.path);
        Ok(devices)
    }

    /// Opens a device.
    pub fn open(&self, info: &DeviceInfo) -> Result<Device, Error> {
        self.open_with(info, |transport| Ok(Box::new(transport)))
    }

    /// Opens a device, letting `wrap` wrap the transport first (for example
    /// in a [`Recorder`](crate::Recorder)).
    pub fn open_with(
        &self,
        info: &DeviceInfo,
        wrap: impl FnOnce(HidTransport) -> Result<Box<dyn Transport>, Error>,
    ) -> Result<Device, Error> {
        let device = self
            .api
            .open_path(&info.path)
            .map_err(|source| Error::Open {
                path: info.path_lossy(),
                source,
            })?;
        Device::with_transport(info.model, wrap(HidTransport::new(device))?)
    }

    /// Opens the device at `path`.
    pub fn open_path(&self, path: &CStr) -> Result<Device, Error> {
        let info = self
            .known
            .iter()
            .find(|info| info.path.as_c_str() == path)
            .ok_or_else(|| Error::NotFound(path.to_string_lossy().into_owned()))?;
        self.open(info)
    }

    /// Waits up to `timeout` (forever if `None`) for devices to be plugged
    /// in or unplugged, and returns what changed since the last call to this
    /// method or [`DeviceManager::devices`]. An empty result means the
    /// timeout passed without changes.
    pub fn wait_for_changes(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Vec<DeviceChange>, Error> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if let Some(monitor) = &mut self.monitor {
                // Any hidraw event may be ours; the rescan sorts it out.
                if !monitor.wait(remaining)? {
                    if remaining.is_some_and(|r| r.is_zero())
                        || deadline.is_some_and(|d| Instant::now() >= d)
                    {
                        return Ok(Vec::new());
                    }
                    continue;
                }
            } else {
                thread::sleep(remaining.map_or(RESCAN_INTERVAL, |r| r.min(RESCAN_INTERVAL)));
            }

            let now = self.scan()?;
            let changes = diff(&self.known, &now);
            self.known = now;
            if !changes.is_empty() || remaining.is_some_and(|r| r.is_zero()) {
                return Ok(changes);
            }
        }
    }
}

/// What changed between two path-sorted device lists. A device whose path
/// stays but whose identity changes (unplugged and another plugged in
/// between scans) counts as a disconnect and a connect.
fn diff(before: &[DeviceInfo], after: &[DeviceInfo]) -> Vec<DeviceChange> {
    let mut changes: Vec<DeviceChange> = before
        .iter()
        .filter(|old| !after.contains(old))
        .cloned()
        .map(DeviceChange::Disconnected)
        .collect();
    changes.extend(
        after
            .iter()
            .filter(|new| !before.contains(new))
            .cloned()
            .map(DeviceChange::Connected),
    );
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str, model: Model, serial: &str) -> DeviceInfo {
        DeviceInfo {
            model,
            serial_number: Some(serial.to_owned()),
            path: CString::new(path).unwrap(),
        }
    }

    #[test]
    fn diff_reports_connects_and_disconnects() {
        let a = info("/dev/hidraw0", Model::Mk2, "A");
        let b = info("/dev/hidraw1", Model::Xl, "B");
        let c = info("/dev/hidraw2", Model::Mini, "C");

        assert_eq!(diff(&[a.clone(), b.clone()], &[a.clone(), b.clone()]), []);
        assert_eq!(
            diff(&[a.clone(), b.clone()], &[a.clone(), c.clone()]),
            [
                DeviceChange::Disconnected(b.clone()),
                DeviceChange::Connected(c.clone())
            ]
        );
        assert_eq!(
            diff(&[], std::slice::from_ref(&a)),
            [DeviceChange::Connected(a.clone())]
        );

        // Same path, different device.
        let replaced = info("/dev/hidraw0", Model::Neo, "D");
        assert_eq!(
            diff(std::slice::from_ref(&a), std::slice::from_ref(&replaced)),
            [
                DeviceChange::Disconnected(a),
                DeviceChange::Connected(replaced)
            ]
        );
    }
}
