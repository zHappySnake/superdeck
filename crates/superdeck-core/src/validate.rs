// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules of `docs/profile-format.md` that the types alone don't
//! enforce.

use std::collections::HashSet;

use crate::devices::Devices;
use crate::model::{ActionInstance, CoreAction, Keys, Layout, Position, Profile};

/// Largest `state` an instance may have.
const MAX_STATE: u8 = 1;

/// Checks a profile. The error says what's wrong and where.
pub fn profile(profile: &Profile) -> Result<(), String> {
    let layout = profile.layout;
    if !(1..=Layout::MAX).contains(&layout.columns) || !(1..=Layout::MAX).contains(&layout.rows) {
        return Err(format!(
            "layout {layout} must have 1 to {} columns and rows",
            Layout::MAX
        ));
    }
    if profile.pages.is_empty() {
        return Err("a profile needs at least one page".to_owned());
    }

    let mut page_ids = HashSet::new();
    for (index, page) in profile.pages.iter().enumerate() {
        if !page_ids.insert(page.id) {
            return Err(format!("page {}: duplicate page ID {}", index + 1, page.id));
        }
    }

    let mut checker = Checker {
        layout,
        page_count: profile.pages.len(),
        instance_ids: HashSet::new(),
    };
    for (index, page) in profile.pages.iter().enumerate() {
        checker
            .keys(&page.keys, false)
            .map_err(|e| format!("page {}, {e}", index + 1))?;
    }
    Ok(())
}

struct Checker {
    layout: Layout,
    page_count: usize,
    instance_ids: HashSet<crate::InstanceId>,
}

impl Checker {
    fn keys(&mut self, keys: &Keys, in_folder: bool) -> Result<(), String> {
        for (&position, instance) in keys {
            let at = || format!("key {position}");
            if !self.layout.contains(position) {
                return Err(format!("{}: outside the {} layout", at(), self.layout));
            }
            if in_folder && position == Position::new(0, 0) {
                return Err(format!("{}: reserved for the folder's back key", at()));
            }
            self.instance(instance, false)
                .map_err(|e| format!("{}: {e}", at()))?;
        }
        Ok(())
    }

    fn instance(&mut self, instance: &ActionInstance, is_step: bool) -> Result<(), String> {
        if !self.instance_ids.insert(instance.id) {
            return Err(format!("duplicate action instance ID {}", instance.id));
        }
        if instance.state > MAX_STATE {
            return Err(format!("state {} is not 0 or 1", instance.state));
        }
        if instance.appearance.len() > usize::from(MAX_STATE) + 1 {
            return Err("more than two appearances".to_owned());
        }
        if is_step && instance.appearance.iter().any(|a| !a.is_empty()) {
            return Err("Multi Action steps have no appearance".to_owned());
        }
        let Some(core) = instance.core() else {
            return Ok(());
        };
        if is_step && !allowed_in_steps(core) {
            return Err(format!("{} can't be a Multi Action step", core.name()));
        }
        match core {
            CoreAction::Folder { keys } => {
                self.keys(keys, true).map_err(|e| format!("in folder, {e}"))
            }
            CoreAction::GoToPage { page } if *page >= self.page_count => Err(format!(
                "go-to-page refers to page {}, but the profile has {}",
                page + 1,
                self.page_count
            )),
            CoreAction::MultiAction { steps } => self.steps(steps),
            CoreAction::MultiActionSwitch { steps } => {
                steps.iter().try_for_each(|steps| self.steps(steps))
            }
            CoreAction::Delay { .. } if !is_step => {
                Err("delay is only allowed as a Multi Action step".to_owned())
            }
            CoreAction::Brightness { value, .. } if *value > 100 => {
                Err(format!("brightness value {value} is over 100"))
            }
            _ => Ok(()),
        }
    }

    fn steps(&mut self, steps: &[ActionInstance]) -> Result<(), String> {
        for (index, step) in steps.iter().enumerate() {
            self.instance(step, true)
                .map_err(|e| format!("step {}: {e}", index + 1))?;
        }
        Ok(())
    }
}

fn allowed_in_steps(core: &CoreAction) -> bool {
    !matches!(
        core,
        CoreAction::Folder { .. }
            | CoreAction::MultiAction { .. }
            | CoreAction::MultiActionSwitch { .. }
            | CoreAction::NextPage
            | CoreAction::PreviousPage
            | CoreAction::GoToPage { .. }
            | CoreAction::PageIndicator
    )
}

/// Checks `devices.json`.
pub fn devices(devices: &Devices) -> Result<(), String> {
    let mut owners = std::collections::HashMap::new();
    for (serial, device) in &devices.devices {
        let at = || format!("device {serial}");
        if device.brightness > 100 {
            return Err(format!(
                "{}: brightness {} is over 100",
                at(),
                device.brightness
            ));
        }
        if let Some(default) = device.default_profile
            && !device.profiles.contains(&default)
        {
            return Err(format!(
                "{}: default profile {default} isn't one of its profiles",
                at()
            ));
        }
        for profile in &device.profiles {
            if let Some(other) = owners.insert(*profile, serial) {
                return Err(if other == serial {
                    format!("{}: profile {profile} is listed twice", at())
                } else {
                    format!("profile {profile} belongs to both device {other} and {serial}")
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceSettings;
    use crate::model::{Action, Appearance, BrightnessMode, PluginAction};
    use crate::{PageId, ProfileId};

    fn layout() -> Layout {
        Layout {
            columns: 5,
            rows: 3,
        }
    }

    fn core(action: CoreAction) -> ActionInstance {
        ActionInstance::new(Action::Core(action))
    }

    fn plugin() -> ActionInstance {
        ActionInstance::new(Action::Plugin(PluginAction {
            plugin: "com.example".to_owned(),
            action: "com.example.action".to_owned(),
            settings: serde_json::Map::new(),
            resources: crate::model::Resources::new(),
        }))
    }

    fn with_key(position: Position, instance: ActionInstance) -> Profile {
        let mut profile = Profile::new("Test", layout());
        profile.pages[0].keys.insert(position, instance);
        profile
    }

    fn folder(keys: Vec<(Position, ActionInstance)>) -> ActionInstance {
        core(CoreAction::Folder {
            keys: keys.into_iter().collect(),
        })
    }

    fn multi(steps: Vec<ActionInstance>) -> ActionInstance {
        core(CoreAction::MultiAction { steps })
    }

    #[track_caller]
    fn rejects(profile: &Profile, message: &str) {
        let error = super::profile(profile).unwrap_err();
        assert!(error.contains(message), "{error:?} lacks {message:?}");
    }

    #[test]
    fn a_valid_profile() {
        let nested = folder(vec![(Position::new(4, 2), plugin())]);
        let mut profile = with_key(
            Position::new(0, 0),
            folder(vec![(Position::new(1, 0), nested)]),
        );
        profile.pages[0].keys.insert(
            Position::new(1, 0),
            multi(vec![
                plugin(),
                core(CoreAction::Delay { milliseconds: 100 }),
            ]),
        );
        profile.pages[0]
            .keys
            .insert(Position::new(2, 0), core(CoreAction::GoToPage { page: 0 }));
        assert_eq!(super::profile(&profile), Ok(()));
    }

    #[test]
    fn layout_and_pages() {
        let mut profile = Profile::new(
            "Test",
            Layout {
                columns: 0,
                rows: 3,
            },
        );
        rejects(&profile, "layout");
        profile.layout = Layout {
            columns: 17,
            rows: 3,
        };
        rejects(&profile, "layout");
        profile.layout = layout();
        profile.pages.clear();
        rejects(&profile, "at least one page");
        let id = PageId::new();
        profile.pages = vec![
            crate::Page {
                id,
                keys: Keys::new()
            };
            2
        ];
        rejects(&profile, "duplicate page ID");
    }

    #[test]
    fn positions_and_folders() {
        rejects(
            &with_key(Position::new(5, 0), plugin()),
            "key 5,0: outside the 5 × 3 layout",
        );
        rejects(&with_key(Position::new(0, 3), plugin()), "outside");
        rejects(
            &with_key(
                Position::new(1, 1),
                folder(vec![(Position::new(0, 0), plugin())]),
            ),
            "page 1, key 1,1: in folder, key 0,0: reserved",
        );
        rejects(
            &with_key(
                Position::new(1, 1),
                folder(vec![(Position::new(7, 0), plugin())]),
            ),
            "outside",
        );
    }

    #[test]
    fn instance_ids_are_unique_across_nesting() {
        let inner = plugin();
        let mut copy = plugin();
        copy.id = inner.id;
        let profile = with_key(
            Position::new(0, 0),
            folder(vec![
                (Position::new(1, 0), inner),
                (Position::new(2, 0), copy),
            ]),
        );
        rejects(&profile, "duplicate action instance ID");
    }

    #[test]
    fn states_and_appearance() {
        let mut instance = plugin();
        instance.state = 2;
        rejects(&with_key(Position::new(0, 0), instance), "state 2");
        let mut instance = plugin();
        instance.appearance = vec![Appearance::default(); 3];
        rejects(&with_key(Position::new(0, 0), instance), "more than two");
    }

    #[test]
    fn multi_action_steps() {
        for (step, message) in [
            (folder(vec![]), "folder can't be a Multi Action step"),
            (multi(vec![]), "multi-action can't be"),
            (core(CoreAction::NextPage), "next-page can't be"),
            (core(CoreAction::PageIndicator), "page-indicator can't be"),
        ] {
            rejects(&with_key(Position::new(0, 0), multi(vec![step])), message);
        }
        let mut titled = plugin();
        titled.appearance = vec![Appearance {
            title: Some("x".to_owned()),
            ..Appearance::default()
        }];
        rejects(
            &with_key(Position::new(0, 0), multi(vec![titled])),
            "no appearance",
        );
        let switch = core(CoreAction::MultiActionSwitch {
            steps: [vec![plugin()], vec![folder(vec![])]],
        });
        rejects(&with_key(Position::new(0, 0), switch), "step 1: folder");
        rejects(
            &with_key(
                Position::new(0, 0),
                core(CoreAction::Delay { milliseconds: 5 }),
            ),
            "only allowed as a Multi Action step",
        );
    }

    #[test]
    fn core_action_values() {
        rejects(
            &with_key(Position::new(0, 0), core(CoreAction::GoToPage { page: 1 })),
            "refers to page 2, but the profile has 1",
        );
        rejects(
            &with_key(
                Position::new(0, 0),
                core(CoreAction::Brightness {
                    mode: BrightnessMode::Set,
                    value: 101,
                }),
            ),
            "over 100",
        );
    }

    #[test]
    fn device_rules() {
        let (a, b) = (ProfileId::new(), ProfileId::new());
        let mut devices = Devices::default();
        devices
            .devices
            .insert("one".to_owned(), DeviceSettings::new(vec![a]));
        assert_eq!(super::devices(&devices), Ok(()));

        let mut bad = devices.clone();
        bad.devices.get_mut("one").unwrap().default_profile = Some(b);
        assert!(
            super::devices(&bad)
                .unwrap_err()
                .contains("isn't one of its profiles")
        );

        let mut bad = devices.clone();
        bad.devices.get_mut("one").unwrap().brightness = 101;
        assert!(super::devices(&bad).unwrap_err().contains("over 100"));

        let mut bad = devices.clone();
        bad.devices.get_mut("one").unwrap().profiles.push(a);
        assert!(super::devices(&bad).unwrap_err().contains("listed twice"));

        let mut bad = devices;
        bad.devices
            .insert("two".to_owned(), DeviceSettings::new(vec![a]));
        assert!(
            super::devices(&bad)
                .unwrap_err()
                .contains("both device one and two")
        );
    }
}
