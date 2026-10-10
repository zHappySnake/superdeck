// SPDX-License-Identifier: GPL-3.0-or-later

//! The raw HID operations a device needs. Keeping them behind a trait lets
//! the same driver code run against real hardware, a recording wrapper, or a
//! replayed capture in tests.

use std::time::Duration;

use hidapi::HidDevice;

use crate::Error;

/// Raw HID access to one device. Every report starts with its report ID.
pub trait Transport: Send {
    /// Sends an output report.
    fn write(&mut self, report: &[u8]) -> Result<(), Error>;

    /// Sends a feature report.
    fn send_feature_report(&mut self, report: &[u8]) -> Result<(), Error>;

    /// Reads feature report `report_id`, `len` bytes long including the ID.
    fn get_feature_report(&mut self, report_id: u8, len: usize) -> Result<Vec<u8>, Error>;

    /// Reads one input report of at most `len` bytes. Returns `None` if
    /// nothing arrived within `timeout`; `None` as timeout blocks.
    fn read(&mut self, len: usize, timeout: Option<Duration>) -> Result<Option<Vec<u8>>, Error>;
}

/// A device opened through hidapi.
pub struct HidTransport {
    device: HidDevice,
}

impl HidTransport {
    pub(crate) fn new(device: HidDevice) -> Self {
        Self { device }
    }
}

impl Transport for HidTransport {
    fn write(&mut self, report: &[u8]) -> Result<(), Error> {
        let written = self.device.write(report)?;
        if written == report.len() {
            Ok(())
        } else {
            Err(Error::ShortTransfer {
                expected: report.len(),
                actual: written,
            })
        }
    }

    fn send_feature_report(&mut self, report: &[u8]) -> Result<(), Error> {
        Ok(self.device.send_feature_report(report)?)
    }

    fn get_feature_report(&mut self, report_id: u8, len: usize) -> Result<Vec<u8>, Error> {
        let mut buf = vec![0; len];
        buf[0] = report_id;
        let read = self.device.get_feature_report(&mut buf)?;
        buf.truncate(read);
        Ok(buf)
    }

    fn read(&mut self, len: usize, timeout: Option<Duration>) -> Result<Option<Vec<u8>>, Error> {
        let timeout_ms = timeout.map_or(-1, |t| i32::try_from(t.as_millis()).unwrap_or(i32::MAX));
        let mut buf = vec![0; len];
        let read = self.device.read_timeout(&mut buf, timeout_ms)?;
        if read == 0 {
            return Ok(None);
        }
        buf.truncate(read);
        Ok(Some(buf))
    }
}
