// SPDX-License-Identifier: GPL-3.0-or-later

//! What the host sends matches the protocol's JSON Schemas in
//! `sdk/protocol/schemas/`.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};
use superdeck_plugin_host::message::{
    ActionEvent, ActionPayload, Coordinates, DeviceInfo, Event, FontStyle, GlobalSettingsPayload,
    InspectorEvent, Instance, Resources, Settings, Size, TitleAlignment, TitleEvent,
    TitleParameters, TitlePayload,
};

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/protocol/schemas")
}

/// Validates `value` against one of the schemas.
#[track_caller]
fn assert_valid(schema: &str, value: &Value) {
    if let Err(e) = validate(schema, value) {
        panic!("{schema} rejects {value:#}:\n{e}");
    }
}

fn validate(schema: &str, value: &Value) -> Result<(), String> {
    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    // The schemas refer to each other by their $id.
    for entry in fs::read_dir(schema_dir()).unwrap() {
        let path = entry.unwrap().path();
        let document: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let id = document["$id"].as_str().unwrap().to_owned();
        compiler.add_resource(&id, document).unwrap();
    }
    let id = format!("https://github.com/zhappysnake/superdeck/sdk/protocol/schemas/{schema}");
    let index = compiler.compile(&id, &mut schemas).unwrap();
    schemas.validate(value, index).map_err(|e| format!("{e:#}"))
}

fn json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn settings() -> Settings {
    match json!({ "volume": 3, "nested": { "a": [1, 2] } }) {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

fn on_key() -> ActionPayload {
    ActionPayload {
        settings: settings(),
        coordinates: Some(Coordinates { column: 4, row: 2 }),
        state: Some(1),
        user_desired_state: None,
        resources: Resources::from([("sound".to_owned(), "/home/u/a.wav".to_owned())]),
    }
}

fn in_multi_action(user_desired_state: Option<u8>) -> ActionPayload {
    ActionPayload {
        settings: Settings::new(),
        coordinates: None,
        state: None,
        user_desired_state,
        resources: Resources::new(),
    }
}

fn action_event(payload: ActionPayload) -> ActionEvent {
    ActionEvent {
        action: "com.example.counter.increment".to_owned(),
        context: "5c0a7e4e-9a42-4e0f-b3c5-0d6c8e2f1b7a".to_owned(),
        device: "6B8F0E25A31C4D7E9F2B8A6C5D4E3F21".to_owned(),
        payload,
    }
}

fn instance() -> Instance {
    Instance {
        action: "com.example.counter.increment".to_owned(),
        context: "c".to_owned(),
        device: "d".to_owned(),
    }
}

#[test]
fn plugin_events() {
    let events = [
        Event::DeviceDidConnect {
            device: "d".to_owned(),
            device_info: DeviceInfo {
                name: "Stream Deck XL".to_owned(),
                size: Size {
                    columns: 8,
                    rows: 4,
                },
                kind: 2,
            },
        },
        Event::DeviceDidDisconnect {
            device: "d".to_owned(),
        },
        Event::WillAppear(action_event(on_key())),
        Event::WillAppear(action_event(in_multi_action(None))),
        Event::WillDisappear(action_event(on_key())),
        Event::KeyDown(action_event(on_key())),
        Event::KeyUp(action_event(in_multi_action(Some(0)))),
        Event::DidReceiveSettings {
            about: action_event(on_key()),
            id: Some("42".to_owned()),
        },
        Event::DidReceiveSettings {
            about: action_event(in_multi_action(None)),
            id: None,
        },
        Event::DidReceiveResources {
            about: action_event(on_key()),
            id: None,
        },
        Event::DidReceiveGlobalSettings {
            payload: GlobalSettingsPayload {
                settings: settings(),
            },
            id: Some("1".to_owned()),
        },
        Event::TitleParametersDidChange(TitleEvent {
            action: "com.example.counter.increment".to_owned(),
            context: "c".to_owned(),
            device: "d".to_owned(),
            payload: TitlePayload {
                settings: settings(),
                coordinates: Coordinates { column: 0, row: 1 },
                state: None,
                title: "3".to_owned(),
                title_parameters: TitleParameters {
                    font_family: String::new(),
                    font_size: 12.0,
                    font_style: FontStyle::BoldItalic,
                    font_underline: false,
                    show_title: true,
                    title_alignment: TitleAlignment::Middle,
                    title_color: "#ffffff".to_owned(),
                },
                resources: Resources::new(),
            },
        }),
        Event::PropertyInspectorDidAppear(instance()),
        Event::PropertyInspectorDidDisappear(instance()),
        Event::SendToPlugin {
            action: "a.b".to_owned(),
            context: "c".to_owned(),
            payload: json!(["anything", 1]),
        },
        Event::SystemDidWakeUp,
    ];
    for event in events {
        assert_valid("plugin-events.schema.json", &json(&event));
    }
    // userDesiredState belongs to keyDown and keyUp only.
    let wrong = Event::WillAppear(action_event(in_multi_action(Some(1))));
    assert!(validate("plugin-events.schema.json", &json(&wrong)).is_err());
}

#[test]
fn inspector_events() {
    let events = [
        InspectorEvent::DidReceiveSettings {
            about: action_event(on_key()),
            id: None,
        },
        InspectorEvent::DidReceiveResources {
            about: action_event(on_key()),
            id: Some("x".to_owned()),
        },
        InspectorEvent::DidReceiveGlobalSettings {
            payload: GlobalSettingsPayload {
                settings: Settings::new(),
            },
            id: None,
        },
        InspectorEvent::SendToPropertyInspector {
            action: "a.b".to_owned(),
            context: "c".to_owned(),
            payload: json!({ "x": null }),
        },
    ];
    for event in events {
        assert_valid("inspector-events.schema.json", &json(&event));
    }
}

/// Commands as the official SDK sends them are accepted by the schemas the
/// host's parser follows.
#[test]
fn sdk_commands() {
    for command in [
        json!({ "event": "setTitle", "context": "c", "payload": { "title": "x", "target": 0 } }),
        json!({ "event": "setImage", "context": "c", "payload": { "image": "data:image/png;base64,AA", "state": 1 } }),
        json!({ "event": "getSettings", "context": "c", "id": "8f0b" }),
        json!({ "event": "getGlobalSettings", "context": "token", "id": "8f0b" }),
        json!({ "event": "switchToProfile", "context": "token", "device": "d", "payload": { "profile": "p", "page": 1 } }),
        json!({ "event": "sendToPropertyInspector", "context": "c", "payload": { "a": 1 } }),
        json!({ "event": "showOk", "context": "c" }),
    ] {
        assert_valid("plugin-commands.schema.json", &command);
    }
    for command in [
        json!({ "event": "setSettings", "context": "c", "payload": {} }),
        json!({ "event": "sendToPlugin", "action": "a.b", "context": "c", "payload": 3 }),
    ] {
        assert_valid("inspector-commands.schema.json", &command);
    }
}
