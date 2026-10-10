// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading and writing real files: the sample files in `tests/data/`, the
//! JSON Schemas in `schema/`, and the configuration directory.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use superdeck_core::model::{BrightnessMode, Color, FontSize, TitleAlignment};
use superdeck_core::{
    Action, ActionInstance, Appearance, ConfigDir, CoreAction, DeviceSettings, Devices, Error,
    Layout, Page, PluginAction, Position, Profile, ProfileId,
};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn sample(name: &str) -> String {
    fs::read_to_string(manifest_dir().join("tests/data/v1").join(name)).unwrap()
}

/// Validates `value` against a schema in `schema/`.
fn schema_errors(schema: &str, value: &Value) -> Option<String> {
    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    let path = manifest_dir().join("schema").join(schema);
    let index = compiler
        .compile(path.to_str().unwrap(), &mut schemas)
        .unwrap();
    schemas
        .validate(value, index)
        .err()
        .map(|e| format!("{e:#}"))
}

#[track_caller]
fn assert_valid(schema: &str, value: &Value) {
    if let Some(errors) = schema_errors(schema, value) {
        panic!("{schema} rejects {value:#}:\n{errors}");
    }
}

/// A config directory holding the sample files, as their IDs require.
fn sample_config() -> (tempfile::TempDir, ConfigDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let profile_dir = config.profile_dir("7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap());
    fs::create_dir_all(&profile_dir).unwrap();
    fs::write(profile_dir.join("profile.json"), sample("profile.json")).unwrap();
    fs::write(temp.path().join("devices.json"), sample("devices.json")).unwrap();
    (temp, config)
}

fn plugin_action(settings: Value) -> ActionInstance {
    let Value::Object(settings) = settings else {
        panic!("settings must be an object");
    };
    ActionInstance::new(Action::Plugin(PluginAction {
        plugin: "com.example".to_owned(),
        action: "com.example.action".to_owned(),
        settings,
    }))
}

fn core(action: CoreAction) -> ActionInstance {
    ActionInstance::new(Action::Core(action))
}

/// A profile that uses every kind of value.
fn rich_profile() -> Profile {
    let mut profile = Profile::new(
        "Rich",
        Layout {
            columns: 8,
            rows: 4,
        },
    );
    profile.applications = vec!["org.mozilla.firefox".to_owned()];
    let mut styled = plugin_action(json!({ "z": 1, "a": [true, null] }));
    styled.state = 1;
    styled.appearance = vec![
        Appearance::default(),
        Appearance {
            title: Some("Two\nlines".to_owned()),
            show_title: Some(false),
            title_alignment: Some(TitleAlignment::Top),
            font_family: Some("Noto Sans".to_owned()),
            font_size: FontSize::new(20.5),
            font_style: None,
            font_underline: Some(false),
            title_color: Some(Color([0, 128, 255])),
            image: None,
        },
    ];
    let folder = core(CoreAction::Folder {
        keys: [(Position::new(7, 3), plugin_action(json!({})))].into(),
    });
    let steps = vec![
        plugin_action(json!({ "n": 1 })),
        core(CoreAction::Delay { milliseconds: 5 }),
        core(CoreAction::Brightness {
            mode: BrightnessMode::Set,
            value: 0,
        }),
    ];
    let keys = &mut profile.pages[0].keys;
    keys.insert(Position::new(0, 0), styled);
    keys.insert(Position::new(1, 0), folder);
    keys.insert(Position::new(2, 0), core(CoreAction::MultiAction { steps }));
    keys.insert(
        Position::new(3, 0),
        core(CoreAction::SwitchProfile {
            profile: ProfileId::new(),
            page: 0,
        }),
    );
    keys.insert(Position::new(4, 0), core(CoreAction::GoToPage { page: 1 }));
    profile.pages.push(Page::new());
    profile
}

#[test]
fn samples_match_the_schemas() {
    assert_valid(
        "profile.v1.schema.json",
        &serde_json::from_str(&sample("profile.json")).unwrap(),
    );
    assert_valid(
        "devices.v1.schema.json",
        &serde_json::from_str(&sample("devices.json")).unwrap(),
    );
}

#[test]
fn samples_load_and_save_unchanged() {
    let (temp, config) = sample_config();
    let profile = config
        .load_profile("7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap())
        .unwrap();
    assert_eq!(profile.migrated_from, None);
    assert_eq!(profile.value.pages.len(), 3);
    assert_eq!(profile.value.instances().count(), 15);
    let devices = config.load_devices().unwrap().value;
    assert_eq!(devices.devices.len(), 2);

    // Writing them back gives the same text: the samples are in the
    // canonical form, field order and all.
    config.save_profile(&profile.value).unwrap();
    config.save_devices(&devices).unwrap();
    let written = |path: PathBuf| fs::read_to_string(path).unwrap();
    assert_eq!(
        written(config.profile_dir(profile.value.id).join("profile.json")),
        sample("profile.json")
    );
    assert_eq!(
        written(temp.path().join("devices.json")),
        sample("devices.json")
    );
}

#[test]
fn written_files_match_the_schemas() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let profile = rich_profile();
    config.save_profile(&profile).unwrap();
    let mut devices = Devices::default();
    let mut device = DeviceSettings::new(vec![profile.id]);
    device.brightness = 30;
    device.sleep_after_minutes = Some(0);
    devices.devices.insert("serial".to_owned(), device);
    config.save_devices(&devices).unwrap();

    let read = |path: PathBuf| -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    };
    assert_valid(
        "profile.v1.schema.json",
        &read(config.profile_dir(profile.id).join("profile.json")),
    );
    assert_valid(
        "devices.v1.schema.json",
        &read(temp.path().join("devices.json")),
    );
    assert_eq!(config.load_profile(profile.id).unwrap().value, profile);
    assert_eq!(config.load_devices().unwrap().value, devices);
}

#[test]
fn plugin_settings_keep_their_order() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let profile = rich_profile();
    config.save_profile(&profile).unwrap();
    let text = fs::read_to_string(config.profile_dir(profile.id).join("profile.json")).unwrap();
    assert!(text.find("\"z\"").unwrap() < text.find("\"a\"").unwrap());
}

#[test]
fn schema_rejects_what_the_code_rejects() {
    let base: Value = serde_json::from_str(&sample("profile.json")).unwrap();
    let key = "/pages/0/keys/0,0";
    let cases: &[(&str, &str, Value)] = &[
        ("unknown field", "/extra", json!(1)),
        (
            "uppercase ID",
            "/id",
            json!("7F9C2E1A-3B4D-4E5F-8A6B-7C8D9E0F1A2B"),
        ),
        ("no pages", "/pages", json!([])),
        (
            "bad position",
            "/pages/0/keys/01,0",
            base.pointer(key).unwrap().clone(),
        ),
        ("state 2", &format!("{key}/state"), json!(2)),
        (
            "font size",
            &format!("{key}/appearance/1/fontSize"),
            json!(200),
        ),
        (
            "font style",
            &format!("{key}/appearance/1/fontStyle"),
            json!("Bold"),
        ),
        (
            "color",
            &format!("{key}/appearance/1/titleColor"),
            json!("red"),
        ),
        (
            "image path",
            &format!("{key}/appearance/0/image"),
            json!("../x.png"),
        ),
        (
            "unknown core action",
            "/pages/0/keys/0,2/action",
            json!("io.github.zhappysnake.superdeck.dance"),
        ),
        (
            "core setting",
            "/pages/0/keys/0,2/settings",
            json!({ "unexpected": 1 }),
        ),
        (
            "brightness",
            "/pages/0/keys/1,0/settings/keys/4,2/settings/value",
            json!(101),
        ),
        ("future version", "/formatVersion", json!(2)),
    ];
    for (what, pointer, value) in cases {
        let mut bad = base.clone();
        set(&mut bad, pointer, value.clone());
        assert!(
            schema_errors("profile.v1.schema.json", &bad).is_some(),
            "schema accepts {what}"
        );
        let text = serde_json::to_string(&bad).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path());
        let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
        fs::create_dir_all(config.profile_dir(id)).unwrap();
        fs::write(config.profile_dir(id).join("profile.json"), text).unwrap();
        assert!(config.load_profile(id).is_err(), "code accepts {what}");
    }
}

/// Sets the value at a JSON pointer, creating the last key if needed.
fn set(value: &mut Value, pointer: &str, new: Value) {
    let (parent, key) = pointer.rsplit_once('/').unwrap();
    let parent = value.pointer_mut(parent).unwrap();
    match parent {
        Value::Object(map) => {
            map.insert(key.to_owned(), new);
        }
        Value::Array(items) => items[key.parse::<usize>().unwrap()] = new,
        _ => panic!("{pointer} is inside a scalar"),
    }
}

#[test]
fn missing_files() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path().join("not-yet"));
    assert_eq!(config.load_devices().unwrap().value, Devices::default());
    assert_eq!(config.profile_ids().unwrap(), []);
    let error = config.load_profile(ProfileId::new()).unwrap_err();
    assert!(error.is_not_found(), "{error}");
}

#[test]
fn profile_ids_ignore_other_entries() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let (a, b) = (rich_profile(), rich_profile());
    config.save_profile(&a).unwrap();
    config.save_profile(&b).unwrap();
    let profiles = temp.path().join("profiles");
    fs::create_dir(profiles.join("not-an-id")).unwrap();
    fs::write(profiles.join(ProfileId::new().to_string()), "a file").unwrap();
    let mut expected = vec![a.id, b.id];
    expected.sort_unstable();
    assert_eq!(config.profile_ids().unwrap(), expected);
}

#[test]
fn damaged_files_are_moved_aside() {
    let (temp, config) = sample_config();
    let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
    let path = config.profile_dir(id).join("profile.json");
    fs::write(&path, "{ not json").unwrap();
    let Err(Error::Damaged { moved_to, .. }) = config.load_profile(id) else {
        panic!("expected a damaged file");
    };
    assert!(!path.exists());
    assert_eq!(fs::read_to_string(&moved_to).unwrap(), "{ not json");
    assert!(
        moved_to
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("profile.json.broken-")
    );

    // A file that parses but breaks a rule, or sits in the wrong directory.
    let mut wrong_id = sample("profile.json");
    wrong_id = wrong_id.replacen("7f9c2e1a", "8f9c2e1a", 1);
    fs::write(&path, wrong_id).unwrap();
    let error = config.load_profile(id).unwrap_err();
    assert!(
        error.to_string().contains("doesn't match its directory"),
        "{error}"
    );

    fs::write(temp.path().join("devices.json"), "[]").unwrap();
    assert!(matches!(config.load_devices(), Err(Error::Damaged { .. })));
}

#[test]
fn newer_files_are_left_alone() {
    let (temp, config) = sample_config();
    let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
    let path = config.profile_dir(id).join("profile.json");
    let newer = r#"{ "formatVersion": 2, "somethingNew": true }"#;
    fs::write(&path, newer).unwrap();
    assert!(matches!(
        config.load_profile(id),
        Err(Error::TooNew {
            found: 2,
            supported: 1,
            ..
        })
    ));
    // Neither loading nor saving touches it.
    let mut profile = Profile::new(
        "Mine",
        Layout {
            columns: 5,
            rows: 3,
        },
    );
    profile.id = id;
    assert!(matches!(
        config.save_profile(&profile),
        Err(Error::TooNew { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), newer);

    fs::write(temp.path().join("devices.json"), newer).unwrap();
    assert!(matches!(config.load_devices(), Err(Error::TooNew { .. })));
}

#[test]
fn unreadable_files_are_not_overwritten() {
    let (_temp, config) = sample_config();
    let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
    let path = config.profile_dir(id).join("profile.json");
    fs::write(&path, "garbage").unwrap();
    let mut profile = Profile::new(
        "Mine",
        Layout {
            columns: 5,
            rows: 3,
        },
    );
    profile.id = id;
    assert!(matches!(
        config.save_profile(&profile),
        Err(Error::Invalid { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "garbage");
}

#[test]
fn invalid_profiles_are_not_saved() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let mut profile = rich_profile();
    profile.pages.clear();
    let error = config.save_profile(&profile).unwrap_err();
    assert!(matches!(error, Error::Invalid { .. }), "{error}");
    assert!(!config.profile_dir(profile.id).exists());
}

#[test]
fn images_are_stored_once_and_cleaned_up() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let mut profile = rich_profile();
    let png = b"\x89PNG\r\n\x1a\nnot really a png";
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    let first = config.add_image(profile.id, png).unwrap();
    assert_eq!(config.add_image(profile.id, png).unwrap(), first);
    let second = config.add_image(profile.id, svg).unwrap();
    let extension = |image: &superdeck_core::model::ImagePath| {
        Path::new(image.as_str()).extension().unwrap().to_owned()
    };
    assert_eq!(
        (extension(&first), extension(&second)),
        ("png".into(), "svg".into())
    );
    assert_eq!(
        fs::read(config.image_file(profile.id, &first)).unwrap(),
        png
    );
    assert!(matches!(
        config.add_image(profile.id, b"BM bitmap"),
        Err(Error::UnsupportedImage)
    ));

    // Only images a key uses survive a save; unrelated files stay.
    let images = config.profile_dir(profile.id).join("images");
    fs::write(images.join("notes.txt"), "mine").unwrap();
    let key = profile.pages[0].keys.get_mut(&Position::new(0, 0)).unwrap();
    key.appearance[0].image = Some(first.clone());
    config.save_profile(&profile).unwrap();
    assert!(config.image_file(profile.id, &first).exists());
    assert!(!config.image_file(profile.id, &second).exists());
    assert!(images.join("notes.txt").exists());

    // Images used inside folders count too.
    let folder = profile.pages[0].keys.get_mut(&Position::new(1, 0)).unwrap();
    let Action::Core(CoreAction::Folder { keys }) = &mut folder.action else {
        panic!("folder expected");
    };
    keys.get_mut(&Position::new(7, 3)).unwrap().appearance = vec![Appearance {
        image: Some(first.clone()),
        ..Appearance::default()
    }];
    profile.pages[0]
        .keys
        .get_mut(&Position::new(0, 0))
        .unwrap()
        .appearance[0]
        .image = None;
    config.save_profile(&profile).unwrap();
    assert!(config.image_file(profile.id, &first).exists());
}

#[test]
fn a_missing_image_file_does_not_stop_loading() {
    let (_temp, config) = sample_config();
    // The sample refers to images that aren't there.
    let id: ProfileId = "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b".parse().unwrap();
    let profile = config.load_profile(id).unwrap().value;
    let image = profile.pages[0].keys[&Position::new(0, 0)].appearance[0]
        .image
        .clone()
        .unwrap();
    assert!(!config.image_file(id, &image).exists());
    config.save_profile(&profile).unwrap();
}

#[test]
fn deleting_a_profile() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let profile = rich_profile();
    config.save_profile(&profile).unwrap();
    config.add_image(profile.id, b"GIF89a").unwrap();
    config.delete_profile(profile.id).unwrap();
    assert!(!config.profile_dir(profile.id).exists());
    config.delete_profile(profile.id).unwrap();
}

#[test]
fn saving_leaves_no_temporary_files() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let profile = rich_profile();
    for _ in 0..3 {
        config.save_profile(&profile).unwrap();
    }
    let names: Vec<_> = fs::read_dir(config.profile_dir(profile.id))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["profile.json"]);
}

#[test]
fn devices_are_checked_before_saving() {
    let temp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(temp.path());
    let mut devices = Devices::default();
    let mut device = DeviceSettings::new(vec![]);
    device.default_profile = Some(ProfileId::new());
    devices.devices.insert("x".to_owned(), device);
    assert!(matches!(
        config.save_devices(&devices),
        Err(Error::Invalid { .. })
    ));
}
