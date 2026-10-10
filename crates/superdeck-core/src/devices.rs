// SPDX-License-Identifier: GPL-3.0-or-later

//! `devices.json`: which profiles each device uses, and its settings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::ProfileId;

/// Every device SuperDeck has seen, by serial number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Devices {
    /// Devices by serial number.
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceSettings>,
}

/// One device's profiles and settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceSettings {
    /// Name shown in the device selector; the model name if `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The device's profiles, in the order the profile list shows.
    pub profiles: Vec<ProfileId>,
    /// Profile shown at start and when no smart profile matches; the first
    /// profile if `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<ProfileId>,
    /// Backlight, 0 to 100.
    #[serde(
        default = "default_brightness",
        skip_serializing_if = "is_default_brightness"
    )]
    pub brightness: u8,
    /// Turn the screen off after this many idle minutes; never if `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_after_minutes: Option<u32>,
}

/// Brightness of a device until the user changes it.
pub const DEFAULT_BRIGHTNESS: u8 = 60;

fn default_brightness() -> u8 {
    DEFAULT_BRIGHTNESS
}

#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde's signature")]
fn is_default_brightness(brightness: &u8) -> bool {
    *brightness == DEFAULT_BRIGHTNESS
}

impl DeviceSettings {
    /// Settings for a device with these profiles, otherwise default.
    #[must_use]
    pub fn new(profiles: Vec<ProfileId>) -> Self {
        Self {
            name: None,
            profiles,
            default_profile: None,
            brightness: DEFAULT_BRIGHTNESS,
            sleep_after_minutes: None,
        }
    }

    /// The profile to show at start: `default_profile`, else the first.
    #[must_use]
    pub fn start_profile(&self) -> Option<ProfileId> {
        self.default_profile
            .or_else(|| self.profiles.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_are_left_out_and_filled_in() {
        let id = ProfileId::new();
        let settings = DeviceSettings::new(vec![id]);
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value, json!({ "profiles": [id.to_string()] }));
        let back: DeviceSettings = serde_json::from_value(value).unwrap();
        assert_eq!(back.brightness, DEFAULT_BRIGHTNESS);
        assert_eq!(back.start_profile(), Some(id));
    }
}
