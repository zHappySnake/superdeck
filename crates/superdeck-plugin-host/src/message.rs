// SPDX-License-Identifier: GPL-3.0-or-later

//! The messages of the protocol, as in `sdk/protocol/schemas/`: events
//! SuperDeck sends, and the commands plugins and property inspectors send
//! back.

use std::collections::BTreeMap;

use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Settings: any JSON object, kept as given.
pub type Settings = Map<String, Value>;

/// Resources: names mapped to file paths.
pub type Resources = BTreeMap<String, String>;

/// A key position, counted from 0 at the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Coordinates {
    /// Column from the left.
    pub column: u8,
    /// Row from the top.
    pub row: u8,
}

/// Key grid of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Size {
    /// Keys per row.
    pub columns: u8,
    /// Rows of keys.
    pub rows: u8,
}

/// A device, as plugins see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    /// The name shown to the user.
    pub name: String,
    /// Key grid.
    pub size: Size,
    /// The SDK's device type: 0 Stream Deck, 1 Mini, 2 XL, 9 Neo.
    #[serde(rename = "type")]
    pub kind: u8,
}

/// A device and its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Device ID.
    pub id: String,
    /// What plugins are told about it.
    pub info: DeviceInfo,
}

/// What an event says about an action instance.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionPayload {
    /// The instance's settings.
    pub settings: Settings,
    /// The key, or `None` for a step of a Multi Action.
    pub coordinates: Option<Coordinates>,
    /// The state, for actions with two states.
    pub state: Option<u8>,
    /// For `keyDown` and `keyUp` of a Multi Action step: the state the user
    /// chose for the step.
    pub user_desired_state: Option<u8>,
    /// The instance's resources.
    pub resources: Resources,
}

impl Serialize for ActionPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("ActionPayload", 7)?;
        s.serialize_field("settings", &self.settings)?;
        if let Some(coordinates) = &self.coordinates {
            s.serialize_field("coordinates", coordinates)?;
        }
        s.serialize_field("controller", "Keypad")?;
        s.serialize_field("isInMultiAction", &self.coordinates.is_none())?;
        if let Some(state) = self.state {
            s.serialize_field("state", &state)?;
        }
        if let Some(state) = self.user_desired_state {
            s.serialize_field("userDesiredState", &state)?;
        }
        s.serialize_field("resources", &self.resources)?;
        s.end()
    }
}

/// An event about an action instance.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionEvent {
    /// Action UUID.
    pub action: String,
    /// The instance.
    pub context: String,
    /// Device ID.
    pub device: String,
    /// About the instance.
    pub payload: ActionPayload,
}

/// Identifies an action instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instance {
    /// Action UUID.
    pub action: String,
    /// The instance.
    pub context: String,
    /// Device ID.
    pub device: String,
}

/// Vertical position of a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[expect(missing_docs, reason = "self-explanatory")]
pub enum TitleAlignment {
    Top,
    Middle,
    Bottom,
}

/// Style of a title's font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[expect(missing_docs, reason = "self-explanatory")]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    #[serde(rename = "Bold Italic")]
    BoldItalic,
}

/// How a title is drawn.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleParameters {
    /// Font family; empty for the default font.
    pub font_family: String,
    /// Pixels on a 72-pixel key.
    pub font_size: f64,
    /// Font style.
    pub font_style: FontStyle,
    /// Underlined.
    pub font_underline: bool,
    /// Whether the title is drawn.
    pub show_title: bool,
    /// Vertical position.
    pub title_alignment: TitleAlignment,
    /// `#rrggbb`, lowercase.
    pub title_color: String,
}

/// What `titleParametersDidChange` says.
#[derive(Debug, Clone, PartialEq)]
pub struct TitlePayload {
    /// The instance's settings.
    pub settings: Settings,
    /// The key.
    pub coordinates: Coordinates,
    /// The state, for actions with two states.
    pub state: Option<u8>,
    /// The title showing.
    pub title: String,
    /// How it's drawn.
    pub title_parameters: TitleParameters,
    /// The instance's resources.
    pub resources: Resources,
}

impl Serialize for TitlePayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("TitlePayload", 7)?;
        s.serialize_field("settings", &self.settings)?;
        s.serialize_field("coordinates", &self.coordinates)?;
        s.serialize_field("controller", "Keypad")?;
        if let Some(state) = self.state {
            s.serialize_field("state", &state)?;
        }
        s.serialize_field("title", &self.title)?;
        s.serialize_field("titleParameters", &self.title_parameters)?;
        s.serialize_field("resources", &self.resources)?;
        s.end()
    }
}

/// `titleParametersDidChange`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TitleEvent {
    /// Action UUID.
    pub action: String,
    /// The instance.
    pub context: String,
    /// Device ID.
    pub device: String,
    /// The title.
    pub payload: TitlePayload,
}

/// The payload of `didReceiveGlobalSettings`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalSettingsPayload {
    /// The plugin's global settings.
    pub settings: Settings,
}

/// An event for a plugin, from `plugin-events.schema.json`. The host
/// sends `deviceDidConnect`, `didReceiveGlobalSettings`, `sendToPlugin`,
/// and the property inspector events itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
#[non_exhaustive]
pub enum Event {
    /// A device was plugged in.
    #[serde(rename_all = "camelCase")]
    DeviceDidConnect {
        /// Device ID.
        device: String,
        /// The device.
        device_info: DeviceInfo,
    },
    /// A device was unplugged.
    DeviceDidDisconnect {
        /// Device ID.
        device: String,
    },
    /// An instance started showing.
    WillAppear(ActionEvent),
    /// An instance stopped showing.
    WillDisappear(ActionEvent),
    /// A key was pressed.
    KeyDown(ActionEvent),
    /// A key was released.
    KeyUp(ActionEvent),
    /// An instance's settings.
    DidReceiveSettings {
        /// The instance.
        #[serde(flatten)]
        about: ActionEvent,
        /// The `id` of the `getSettings` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// An instance's resources.
    DidReceiveResources {
        /// The instance.
        #[serde(flatten)]
        about: ActionEvent,
        /// The `id` of the `getResources` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// The plugin's global settings.
    DidReceiveGlobalSettings {
        /// The settings.
        payload: GlobalSettingsPayload,
        /// The `id` of the `getGlobalSettings` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A title, or how it's drawn, changed.
    TitleParametersDidChange(TitleEvent),
    /// The instance's property inspector opened.
    PropertyInspectorDidAppear(Instance),
    /// The instance's property inspector closed.
    PropertyInspectorDidDisappear(Instance),
    /// The property inspector sent a message.
    SendToPlugin {
        /// Action UUID.
        action: String,
        /// The instance.
        context: String,
        /// As sent.
        payload: Value,
    },
    /// The computer resumed from suspend.
    SystemDidWakeUp,
}

impl Event {
    /// The instance the event is about, if any.
    #[must_use]
    pub fn context(&self) -> Option<&str> {
        match self {
            Event::WillAppear(event)
            | Event::WillDisappear(event)
            | Event::KeyDown(event)
            | Event::KeyUp(event)
            | Event::DidReceiveSettings { about: event, .. }
            | Event::DidReceiveResources { about: event, .. } => Some(&event.context),
            Event::TitleParametersDidChange(event) => Some(&event.context),
            Event::PropertyInspectorDidAppear(instance)
            | Event::PropertyInspectorDidDisappear(instance) => Some(&instance.context),
            Event::SendToPlugin { context, .. } => Some(context),
            Event::DeviceDidConnect { .. }
            | Event::DeviceDidDisconnect { .. }
            | Event::DidReceiveGlobalSettings { .. }
            | Event::SystemDidWakeUp => None,
        }
    }
}

/// An event for a property inspector, from `inspector-events.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
#[non_exhaustive]
pub enum InspectorEvent {
    /// The instance's settings.
    DidReceiveSettings {
        /// The instance.
        #[serde(flatten)]
        about: ActionEvent,
        /// The `id` of the `getSettings` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// The instance's resources.
    DidReceiveResources {
        /// The instance.
        #[serde(flatten)]
        about: ActionEvent,
        /// The `id` of the `getResources` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// The plugin's global settings.
    DidReceiveGlobalSettings {
        /// The settings.
        payload: GlobalSettingsPayload,
        /// The `id` of the `getGlobalSettings` this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// The plugin sent a message.
    SendToPropertyInspector {
        /// Action UUID.
        action: String,
        /// The instance.
        context: String,
        /// As sent.
        payload: Value,
    },
}

/// The first message on a connection.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Registration {
    pub event: String,
    pub uuid: String,
}

/// A command from a plugin or a property inspector. Fields SuperDeck
/// doesn't use are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub(crate) enum Command {
    SetSettings {
        context: String,
        payload: Settings,
    },
    GetSettings {
        context: String,
        #[serde(default)]
        id: Option<String>,
    },
    SetGlobalSettings {
        payload: Settings,
    },
    GetGlobalSettings {
        #[serde(default)]
        id: Option<String>,
    },
    SetResources {
        context: String,
        payload: Resources,
    },
    GetResources {
        context: String,
        #[serde(default)]
        id: Option<String>,
    },
    OpenUrl {
        payload: UrlPayload,
    },
    LogMessage {
        payload: LogPayload,
    },
    SetTitle {
        context: String,
        #[serde(default)]
        payload: TitleCommand,
    },
    SetImage {
        context: String,
        #[serde(default)]
        payload: ImageCommand,
    },
    SetState {
        context: String,
        payload: StateCommand,
    },
    ShowAlert {
        context: String,
    },
    ShowOk {
        context: String,
    },
    SwitchToProfile {
        device: String,
        #[serde(default)]
        payload: ProfileCommand,
    },
    SendToPropertyInspector {
        context: String,
        payload: Value,
    },
    SendToPlugin {
        context: String,
        payload: Value,
    },
    // Stream Deck +, accepted and ignored.
    SetFeedback,
    SetFeedbackLayout,
    SetTriggerDescription,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct UrlPayload {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct LogPayload {
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct TitleCommand {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub state: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct ImageCommand {
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub state: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct StateCommand {
    pub state: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct ProfileCommand {
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub page: Option<usize>,
}

/// Why a message couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// Not a JSON object with an `event`.
    NotAMessage(String),
    /// An event SuperDeck doesn't know.
    Unknown(String),
    /// A known event, with fields of the wrong shape.
    Invalid { event: String, reason: String },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NotAMessage(reason) => write!(f, "not a message: {reason}"),
            ParseError::Unknown(event) => write!(f, "unknown event {event:?}"),
            ParseError::Invalid { event, reason } => write!(f, "invalid {event}: {reason}"),
        }
    }
}

impl Command {
    /// Reads a command.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| ParseError::NotAMessage(e.to_string()))?;
        let Some(event) = value.get("event").and_then(Value::as_str) else {
            return Err(ParseError::NotAMessage("no event".to_owned()));
        };
        let event = event.to_owned();
        serde_json::from_value(value).map_err(|e| {
            let reason = e.to_string();
            if reason.starts_with("unknown variant") {
                ParseError::Unknown(event)
            } else {
                ParseError::Invalid { event, reason }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn commands() {
        assert_eq!(
            Command::parse(
                r#"{"event":"setTitle","context":"c","payload":{"title":"Hi","target":0,"state":1}}"#
            ),
            Ok(Command::SetTitle {
                context: "c".to_owned(),
                payload: TitleCommand {
                    title: Some("Hi".to_owned()),
                    state: Some(1)
                }
            })
        );
        // Plugins written for the official app send fields we don't use.
        assert_eq!(
            Command::parse(r#"{"event":"showOk","context":"c","action":"x","device":"d"}"#),
            Ok(Command::ShowOk {
                context: "c".to_owned()
            })
        );
        assert_eq!(
            Command::parse(r#"{"event":"setImage","context":"c"}"#),
            Ok(Command::SetImage {
                context: "c".to_owned(),
                payload: ImageCommand::default()
            })
        );
        assert_eq!(
            Command::parse(r#"{"event":"getGlobalSettings","context":"token","id":"1"}"#),
            Ok(Command::GetGlobalSettings {
                id: Some("1".to_owned())
            })
        );
        assert_eq!(
            Command::parse(r#"{"event":"setFeedback","context":"c","payload":{"a":1}}"#),
            Ok(Command::SetFeedback)
        );
        assert_eq!(
            Command::parse(r#"{"event":"selfDestruct"}"#),
            Err(ParseError::Unknown("selfDestruct".to_owned()))
        );
        assert!(matches!(
            Command::parse(r#"{"event":"setState","context":"c","payload":{"state":"on"}}"#),
            Err(ParseError::Invalid { event, .. }) if event == "setState"
        ));
        assert!(matches!(
            Command::parse("[1]"),
            Err(ParseError::NotAMessage(_))
        ));
        assert!(matches!(
            Command::parse("{"),
            Err(ParseError::NotAMessage(_))
        ));
    }

    #[test]
    fn events() {
        let event = Event::KeyDown(ActionEvent {
            action: "com.example.a".to_owned(),
            context: "c".to_owned(),
            device: "D".to_owned(),
            payload: ActionPayload {
                settings: Settings::new(),
                coordinates: None,
                state: Some(1),
                user_desired_state: Some(0),
                resources: Resources::new(),
            },
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "event": "keyDown",
                "action": "com.example.a",
                "context": "c",
                "device": "D",
                "payload": {
                    "settings": {},
                    "controller": "Keypad",
                    "isInMultiAction": true,
                    "state": 1,
                    "userDesiredState": 0,
                    "resources": {}
                }
            })
        );
        assert_eq!(event.context(), Some("c"));
        assert_eq!(
            serde_json::to_value(Event::SystemDidWakeUp).unwrap(),
            json!({ "event": "systemDidWakeUp" })
        );
    }
}
