// SPDX-License-Identifier: GPL-3.0-or-later

//! udev events for hidraw devices.

use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use udev::{EventType, MonitorBuilder, MonitorSocket};

use crate::Error;

/// Listens for hidraw devices being added or removed.
pub(crate) struct HotplugMonitor {
    socket: MonitorSocket,
}

impl HotplugMonitor {
    pub(crate) fn new() -> Result<Self, Error> {
        let socket = MonitorBuilder::new()?.match_subsystem("hidraw")?.listen()?;
        Ok(Self { socket })
    }

    /// Waits up to `timeout` (forever if `None`) for hidraw devices to be
    /// added or removed. Returns whether that happened; `false` can also
    /// mean another kind of event or a signal arrived early.
    ///
    /// Events aren't filtered by vendor: once a device is gone, udev can no
    /// longer tell whose it was. Callers rescan to find out what changed.
    pub(crate) fn wait(&mut self, timeout: Option<Duration>) -> Result<bool, Error> {
        let timeout = timeout.map(|t| {
            Timespec::try_from(t).unwrap_or(Timespec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            })
        });
        let mut fds = [PollFd::new(&self.socket, PollFlags::IN)];
        match poll(&mut fds, timeout.as_ref()) {
            Ok(_) => {}
            // Interrupted by a signal; the caller retries.
            Err(rustix::io::Errno::INTR) => return Ok(false),
            Err(error) => return Err(Error::Io(error.into())),
        }
        // Drain everything that's queued.
        let mut changed = false;
        for event in self.socket.iter() {
            changed |= matches!(event.event_type(), EventType::Add | EventType::Remove);
        }
        Ok(changed)
    }
}
