// SPDX-License-Identifier: GPL-3.0-or-later

//! IDs of profiles, pages, and action instances: random UUIDs, written in
//! lowercase with hyphens. Other spellings are rejected, so an ID has one
//! form in files and directory names.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A new random ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.hyphenated().fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                parse(s).map(Self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(de::Error::custom)
            }
        }
    };
}

/// Parses the canonical spelling only.
fn parse(s: &str) -> Result<Uuid, String> {
    Uuid::parse_str(s)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == s)
        .ok_or_else(|| format!("invalid ID {s:?}, expected a lowercase UUID with hyphens"))
}

id! {
    /// Identifies a profile; also the name of its directory.
    ProfileId
}

id! {
    /// Identifies a page within a profile.
    PageId
}

id! {
    /// Identifies an action instance. Plugins see it as the `context` of
    /// their events.
    InstanceId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_lowercase_uuids() {
        let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
        assert_eq!(id.to_string(), "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b");
        for other in [
            "7F9C2E1A-3B4D-4E5F-8A6B-7C8D9E0F1A2B",
            "7f9c2e1a3b4d4e5f8a6b7c8d9e0f1a2b",
            "urn:uuid:7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b",
            "{7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b}",
        ] {
            assert!(other.parse::<ProfileId>().is_err(), "{other}");
            assert!(
                serde_json::from_value::<ProfileId>(other.into()).is_err(),
                "{other}"
            );
        }
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b\""
        );
        assert_ne!(InstanceId::new(), InstanceId::new());
        assert!("not-a-uuid".parse::<PageId>().is_err());
    }
}
