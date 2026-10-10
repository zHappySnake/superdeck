// SPDX-License-Identifier: GPL-3.0-or-later

//! Profiles, pages, keys, and action instances, as described in
//! `docs/profile-format.md`. Serde maps these types to `profile.json`.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::{InstanceId, PageId, ProfileId};

/// Plugin UUID of the core actions.
pub const CORE_PLUGIN: &str = "io.github.zhappysnake.superdeck";

/// A profile: pages of keys for one key layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    /// Profile ID; also the profile's directory name.
    pub id: ProfileId,
    /// Name shown in the profile list.
    pub name: String,
    /// Key grid the profile is made for.
    pub layout: Layout,
    /// Application IDs that switch to this profile when focused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applications: Vec<String>,
    /// The plugin profile this one was installed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Pages, in order. Never empty.
    pub pages: Vec<Page>,
}

impl Profile {
    /// A new profile with one empty page.
    #[must_use]
    pub fn new(name: impl Into<String>, layout: Layout) -> Self {
        Self {
            id: ProfileId::new(),
            name: name.into(),
            layout,
            applications: Vec::new(),
            source: None,
            pages: vec![Page::new()],
        }
    }

    /// Every action instance in the profile, including those in folders and
    /// Multi Actions, depth first.
    pub fn instances(&self) -> impl Iterator<Item = &ActionInstance> {
        let mut stack: Vec<&ActionInstance> = self
            .pages
            .iter()
            .flat_map(|page| page.keys.values())
            .rev()
            .collect();
        std::iter::from_fn(move || {
            let instance = stack.pop()?;
            stack.extend(instance.children().rev());
            Some(instance)
        })
    }

    /// The action instance with this ID, wherever it is.
    #[must_use]
    pub fn instance(&self, id: InstanceId) -> Option<&ActionInstance> {
        self.instances().find(|instance| instance.id == id)
    }

    /// The action instance with this ID, wherever it is, to change it.
    pub fn instance_mut(&mut self, id: InstanceId) -> Option<&mut ActionInstance> {
        self.pages
            .iter_mut()
            .flat_map(|page| page.keys.values_mut())
            .find_map(|instance| instance.find_mut(id))
    }
}

/// A key grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Keys per row.
    pub columns: u8,
    /// Rows of keys.
    pub rows: u8,
}

impl Layout {
    /// Largest number of columns or rows a layout may have.
    pub const MAX: u8 = 16;

    /// Whether `position` is on the grid.
    #[must_use]
    pub fn contains(self, position: Position) -> bool {
        position.column < self.columns && position.row < self.rows
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} × {}", self.columns, self.rows)
    }
}

/// Where a profile came from, when a plugin shipped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// UUID of the plugin that shipped the profile.
    pub plugin: String,
    /// The profile's name in the plugin's `manifest.json`.
    pub profile: String,
}

/// One page of keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// Page ID.
    pub id: PageId,
    /// Keys that have an action.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: Keys,
}

impl Page {
    /// A new empty page.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: PageId::new(),
            keys: Keys::new(),
        }
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions by key position, in reading order.
pub type Keys = BTreeMap<Position, ActionInstance>;

/// A key position, counted from 0 at the top left. Written as
/// `"column,row"`. Ordered row by row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    // Row first, so the derived order is reading order.
    /// Row from the top.
    pub row: u8,
    /// Column from the left.
    pub column: u8,
}

impl Position {
    /// The key at `column`, `row`.
    #[must_use]
    pub const fn new(column: u8, row: u8) -> Self {
        Self { row, column }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.column, self.row)
    }
}

impl FromStr for Position {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || format!("invalid key position {s:?}, expected \"column,row\"");
        let (column, row) = s.split_once(',').ok_or_else(error)?;
        let position = Self::new(
            column.parse().map_err(|_| error())?,
            row.parse().map_err(|_| error())?,
        );
        // Only the canonical spelling: no signs, spaces, or leading zeros.
        if position.to_string() != s {
            return Err(error());
        }
        Ok(position)
    }
}

impl Serialize for Position {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Position {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// An action placed on a key, with its settings and look.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionInstance {
    /// Instance ID; the SDK `context`.
    pub id: InstanceId,
    /// What the action is, and its settings.
    pub action: Action,
    /// Current state of a multi-state action (0 or 1).
    pub state: u8,
    /// User customizations, one per state, in state order.
    pub appearance: Vec<Appearance>,
}

impl ActionInstance {
    /// A new instance with a fresh ID, in state 0, not customized.
    #[must_use]
    pub fn new(action: Action) -> Self {
        Self {
            id: InstanceId::new(),
            action,
            state: 0,
            appearance: Vec::new(),
        }
    }

    /// The core action, if this is one.
    #[must_use]
    pub fn core(&self) -> Option<&CoreAction> {
        match &self.action {
            Action::Core(core) => Some(core),
            Action::Plugin(_) => None,
        }
    }

    /// This instance or one inside it with the given ID.
    fn find_mut(&mut self, id: InstanceId) -> Option<&mut ActionInstance> {
        if self.id == id {
            return Some(self);
        }
        match &mut self.action {
            Action::Core(CoreAction::Folder { keys }) => {
                keys.values_mut().find_map(|child| child.find_mut(id))
            }
            Action::Core(CoreAction::MultiAction { steps }) => {
                steps.iter_mut().find_map(|child| child.find_mut(id))
            }
            Action::Core(CoreAction::MultiActionSwitch { steps }) => steps
                .iter_mut()
                .flatten()
                .find_map(|child| child.find_mut(id)),
            _ => None,
        }
    }

    /// Instances directly inside this one: a folder's keys or a Multi
    /// Action's steps.
    pub fn children(&self) -> impl DoubleEndedIterator<Item = &ActionInstance> {
        let (keys, steps): (Option<&Keys>, &[Vec<ActionInstance>]) = match self.core() {
            Some(CoreAction::Folder { keys }) => (Some(keys), &[]),
            Some(CoreAction::MultiAction { steps }) => (None, std::slice::from_ref(steps)),
            Some(CoreAction::MultiActionSwitch { steps }) => (None, steps),
            _ => (None, &[]),
        };
        keys.into_iter()
            .flat_map(BTreeMap::values)
            .chain(steps.iter().flatten())
    }
}

/// The action an instance performs.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// An action from a plugin.
    Plugin(PluginAction),
    /// A built-in action.
    Core(CoreAction),
}

/// An action from a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginAction {
    /// Plugin UUID.
    pub plugin: String,
    /// Action UUID, as in the plugin's `manifest.json`.
    pub action: String,
    /// Settings the plugin saved, kept as given.
    pub settings: Map<String, Value>,
    /// Files the instance refers to, by name (`setResources`).
    pub resources: Resources,
}

/// Files a plugin action refers to: names mapped to paths.
pub type Resources = BTreeMap<String, String>;

/// A built-in action. Its settings are part of the profile format.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreAction {
    /// A folder with its own key grid; its top-left key is the back key.
    Folder {
        /// The folder's keys. `0,0` stays empty.
        keys: Keys,
    },
    /// Show another profile.
    SwitchProfile {
        /// Profile to show.
        profile: ProfileId,
        /// Page of that profile to show.
        page: usize,
    },
    /// Show the next page.
    NextPage,
    /// Show the previous page.
    PreviousPage,
    /// Show a page of this profile.
    GoToPage {
        /// Index into the profile's pages.
        page: usize,
    },
    /// Show the current page number.
    PageIndicator,
    /// Run several actions in order.
    MultiAction {
        /// The actions to run.
        steps: Vec<ActionInstance>,
    },
    /// Two Multi Actions, run in turn.
    MultiActionSwitch {
        /// The steps for each state.
        steps: [Vec<ActionInstance>; 2],
    },
    /// Wait, in a Multi Action.
    Delay {
        /// How long.
        milliseconds: u32,
    },
    /// Change the backlight.
    Brightness {
        /// Set, raise, or lower.
        mode: BrightnessMode,
        /// Percentage to set, or to change by.
        value: u8,
    },
    /// Turn the screen off until a key is pressed.
    Sleep,
}

/// What a Brightness action does with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrightnessMode {
    /// Set the brightness to the value.
    Set,
    /// Raise the brightness by the value.
    Increase,
    /// Lower the brightness by the value.
    Decrease,
}

// Settings of the core actions, as read. Writing builds them directly, so
// nested keys and steps aren't copied.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderSettings {
    #[serde(default)]
    keys: Keys,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchProfileSettings {
    profile: ProfileId,
    #[serde(default)]
    page: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageSettings {
    page: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepsSettings {
    #[serde(default)]
    steps: Vec<ActionInstance>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchStepsSettings {
    #[serde(default)]
    steps: [Vec<ActionInstance>; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DelaySettings {
    milliseconds: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrightnessSettings {
    mode: BrightnessMode,
    value: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoSettings {}

impl CoreAction {
    /// The action's name, after the core plugin prefix.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            CoreAction::Folder { .. } => "folder",
            CoreAction::SwitchProfile { .. } => "switch-profile",
            CoreAction::NextPage => "next-page",
            CoreAction::PreviousPage => "previous-page",
            CoreAction::GoToPage { .. } => "go-to-page",
            CoreAction::PageIndicator => "page-indicator",
            CoreAction::MultiAction { .. } => "multi-action",
            CoreAction::MultiActionSwitch { .. } => "multi-action-switch",
            CoreAction::Delay { .. } => "delay",
            CoreAction::Brightness { .. } => "brightness",
            CoreAction::Sleep => "sleep",
        }
    }

    /// The full action UUID.
    #[must_use]
    pub fn uuid(&self) -> String {
        format!("{CORE_PLUGIN}.{}", self.name())
    }

    fn settings(&self) -> Result<Value, serde_json::Error> {
        let mut settings = Map::new();
        let mut set = |name: &str, value: Value| {
            settings.insert(name.to_owned(), value);
        };
        match self {
            CoreAction::Folder { keys } if !keys.is_empty() => {
                set("keys", serde_json::to_value(keys)?);
            }
            CoreAction::SwitchProfile { profile, page } => {
                set("profile", serde_json::to_value(profile)?);
                if *page != 0 {
                    set("page", (*page).into());
                }
            }
            CoreAction::GoToPage { page } => set("page", (*page).into()),
            CoreAction::MultiAction { steps } if !steps.is_empty() => {
                set("steps", serde_json::to_value(steps)?);
            }
            CoreAction::MultiActionSwitch { steps } => {
                set("steps", serde_json::to_value(steps)?);
            }
            CoreAction::Delay { milliseconds } => set("milliseconds", (*milliseconds).into()),
            CoreAction::Brightness { mode, value } => {
                set("mode", serde_json::to_value(mode)?);
                set("value", (*value).into());
            }
            CoreAction::Folder { .. }
            | CoreAction::MultiAction { .. }
            | CoreAction::NextPage
            | CoreAction::PreviousPage
            | CoreAction::PageIndicator
            | CoreAction::Sleep => {}
        }
        Ok(Value::Object(settings))
    }

    fn from_settings(name: &str, settings: Map<String, Value>) -> Result<Self, String> {
        fn parse<T: de::DeserializeOwned>(
            name: &str,
            settings: Map<String, Value>,
        ) -> Result<T, String> {
            serde_json::from_value(Value::Object(settings))
                .map_err(|e| format!("settings of core action {name:?}: {e}"))
        }
        Ok(match name {
            "folder" => CoreAction::Folder {
                keys: parse::<FolderSettings>(name, settings)?.keys,
            },
            "switch-profile" => {
                let s: SwitchProfileSettings = parse(name, settings)?;
                CoreAction::SwitchProfile {
                    profile: s.profile,
                    page: s.page,
                }
            }
            "go-to-page" => CoreAction::GoToPage {
                page: parse::<PageSettings>(name, settings)?.page,
            },
            "multi-action" => CoreAction::MultiAction {
                steps: parse::<StepsSettings>(name, settings)?.steps,
            },
            "multi-action-switch" => CoreAction::MultiActionSwitch {
                steps: parse::<SwitchStepsSettings>(name, settings)?.steps,
            },
            "delay" => CoreAction::Delay {
                milliseconds: parse::<DelaySettings>(name, settings)?.milliseconds,
            },
            "brightness" => {
                let s: BrightnessSettings = parse(name, settings)?;
                CoreAction::Brightness {
                    mode: s.mode,
                    value: s.value,
                }
            }
            "next-page" | "previous-page" | "page-indicator" | "sleep" => {
                parse::<NoSettings>(name, settings)?;
                match name {
                    "next-page" => CoreAction::NextPage,
                    "previous-page" => CoreAction::PreviousPage,
                    "page-indicator" => CoreAction::PageIndicator,
                    _ => CoreAction::Sleep,
                }
            }
            _ => return Err(format!("unknown core action {name:?}")),
        })
    }
}

/// An action instance as stored, before its action is interpreted.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredInstance {
    id: InstanceId,
    action: String,
    plugin: String,
    #[serde(default)]
    settings: Map<String, Value>,
    #[serde(default)]
    resources: Resources,
    #[serde(default)]
    state: u8,
    #[serde(default)]
    appearance: Vec<Appearance>,
}

impl Serialize for ActionInstance {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let empty = Resources::new();
        let (plugin, action, settings, resources) = match &self.action {
            Action::Plugin(plugin) => (
                plugin.plugin.clone(),
                plugin.action.clone(),
                Value::Object(plugin.settings.clone()),
                &plugin.resources,
            ),
            Action::Core(core) => (
                CORE_PLUGIN.to_owned(),
                core.uuid(),
                core.settings().map_err(serde::ser::Error::custom)?,
                &empty,
            ),
        };
        let has_settings = settings.as_object().is_some_and(|s| !s.is_empty());
        let fields = 3
            + usize::from(has_settings)
            + usize::from(!resources.is_empty())
            + usize::from(self.state != 0)
            + usize::from(!self.appearance.is_empty());
        let mut s = serializer.serialize_struct("ActionInstance", fields)?;
        s.serialize_field("id", &self.id)?;
        s.serialize_field("action", &action)?;
        s.serialize_field("plugin", &plugin)?;
        if has_settings {
            s.serialize_field("settings", &settings)?;
        }
        if !resources.is_empty() {
            s.serialize_field("resources", resources)?;
        }
        if self.state != 0 {
            s.serialize_field("state", &self.state)?;
        }
        if !self.appearance.is_empty() {
            s.serialize_field("appearance", &self.appearance)?;
        }
        s.end()
    }
}

impl<'de> Deserialize<'de> for ActionInstance {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredInstance::deserialize(deserializer)?;
        let action = if stored.plugin == CORE_PLUGIN {
            if !stored.resources.is_empty() {
                return Err(de::Error::custom("core actions have no resources"));
            }
            let name = stored
                .action
                .strip_prefix(CORE_PLUGIN)
                .and_then(|rest| rest.strip_prefix('.'))
                .ok_or_else(|| {
                    de::Error::custom(format!("unknown core action {:?}", stored.action))
                })?;
            Action::Core(
                CoreAction::from_settings(name, stored.settings).map_err(de::Error::custom)?,
            )
        } else {
            Action::Plugin(PluginAction {
                plugin: stored.plugin,
                action: stored.action,
                settings: stored.settings,
                resources: stored.resources,
            })
        };
        Ok(Self {
            id: stored.id,
            action,
            state: stored.state,
            appearance: stored.appearance,
        })
    }
}

/// What the user set in the property inspector for one state. `None`
/// means not customized: the plugin's value or the manifest default applies.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    /// Title text; `\n` separates lines. `Some("")` is an intentionally
    /// empty title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether the title is drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_title: Option<bool>,
    /// Vertical position of the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_alignment: Option<TitleAlignment>,
    /// Font family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Font size in pixels on a 72-pixel key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<FontSize>,
    /// Font style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<FontStyle>,
    /// Underlined title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_underline: Option<bool>,
    /// Title color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_color: Option<Color>,
    /// Custom image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImagePath>,
}

impl Appearance {
    /// Whether nothing is customized.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Vertical position of a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleAlignment {
    /// Along the top edge.
    Top,
    /// Centered.
    Middle,
    /// Along the bottom edge.
    Bottom,
}

/// Font style of a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontStyle {
    /// Regular.
    Regular,
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Bold and italic.
    BoldItalic,
}

/// A title font size: pixels on a 72-pixel key, from 1 to 144. Whole sizes
/// are written as integers.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Deserialize)]
#[serde(try_from = "f64")]
pub struct FontSize(f64);

impl FontSize {
    /// Smallest size.
    pub const MIN: f64 = 1.0;
    /// Largest size.
    pub const MAX: f64 = 144.0;

    /// The size, or `None` if out of range.
    #[must_use]
    pub fn new(size: f64) -> Option<Self> {
        (Self::MIN..=Self::MAX)
            .contains(&size)
            .then_some(Self(size))
    }

    /// The size in pixels on a 72-pixel key.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for FontSize {
    type Error = String;

    fn try_from(size: f64) -> Result<Self, Self::Error> {
        Self::new(size)
            .ok_or_else(|| format!("font size {size} is outside {} to {}", Self::MIN, Self::MAX))
    }
}

impl Serialize for FontSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.fract() == 0.0 {
            #[expect(clippy::cast_possible_truncation, reason = "a whole number up to 144")]
            #[expect(clippy::cast_sign_loss, reason = "at least 1")]
            serializer.serialize_u8(self.0 as u8)
        } else {
            serializer.serialize_f64(self.0)
        }
    }
}

/// An RGB color, written `"#rrggbb"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub [u8; 3]);

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || format!("invalid color {s:?}, expected \"#rrggbb\"");
        let hex = s.strip_prefix('#').ok_or_else(error)?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| error());
        Ok(Self([channel(0)?, channel(2)?, channel(4)?]))
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Path of a custom image inside a profile directory:
/// `images/<sha256>.<ext>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImagePath(String);

/// File extensions of the image formats a profile can hold.
pub const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "gif", "svg"];

impl ImagePath {
    /// Directory, inside the profile directory, that holds images.
    pub const DIR: &str = "images";

    /// The path as stored, relative to the profile directory.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file name, without the directory.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.0[Self::DIR.len() + 1..]
    }

    /// Builds a path from a lowercase hex SHA-256 and an extension.
    pub(crate) fn from_parts(hash: &str, extension: &str) -> Self {
        Self(format!("{}/{hash}.{extension}", Self::DIR))
    }
}

impl FromStr for ImagePath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s
            .strip_prefix("images/")
            .and_then(|name| name.split_once('.'))
            .is_some_and(|(hash, extension)| {
                hash.len() == 64
                    && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    && IMAGE_EXTENSIONS.contains(&extension)
            });
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!(
                "invalid image path {s:?}, expected \"images/<sha256>.<png|jpg|gif|svg>\""
            ))
        }
    }
}

impl Serialize for ImagePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ImagePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn positions() {
        assert_eq!("4,2".parse(), Ok(Position::new(4, 2)));
        assert_eq!(Position::new(10, 0).to_string(), "10,0");
        for bad in [
            "", "1", "1,", ",1", "01,1", "+1,1", "1, 1", "1,1,1", "256,0", "-1,0",
        ] {
            assert!(bad.parse::<Position>().is_err(), "{bad:?}");
        }
        // Reading order: row by row.
        assert!(Position::new(4, 0) < Position::new(0, 1));
    }

    #[test]
    fn colors() {
        assert_eq!("#FF8000".parse(), Ok(Color([255, 128, 0])));
        assert_eq!(Color([255, 128, 0]).to_string(), "#ff8000");
        for bad in ["ff8000", "#ff800", "#ff80000", "#gg8000", "#ff80é"] {
            assert!(bad.parse::<Color>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn image_paths() {
        let hash = "a".repeat(64);
        let path: ImagePath = format!("images/{hash}.png").parse().unwrap();
        assert_eq!(path.file_name(), format!("{hash}.png"));
        for bad in [
            format!("images/{hash}.bmp"),
            format!("images/{}.png", "A".repeat(64)),
            format!("images/{}.png", "a".repeat(63)),
            format!("../{hash}.png"),
            format!("images/../{hash}.png"),
            format!("/images/{hash}.png"),
        ] {
            assert!(bad.parse::<ImagePath>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn font_sizes() {
        let size: FontSize = serde_json::from_value(json!(12)).unwrap();
        assert!((size.get() - 12.0).abs() < f64::EPSILON);
        assert_eq!(serde_json::to_string(&size).unwrap(), "12");
        let size: FontSize = serde_json::from_value(json!(20.3)).unwrap();
        assert_eq!(serde_json::to_string(&size).unwrap(), "20.3");
        for bad in [json!(0), json!(144.5), json!(-1), json!("12")] {
            assert!(
                serde_json::from_value::<FontSize>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn plugin_instance_round_trips() {
        let stored = json!({
            "id": "5c0a7e4e-9a42-4e0f-b3c5-0d6c8e2f1b7a",
            "action": "com.example.plugin.action",
            "plugin": "com.example.plugin",
            "settings": { "anything": [1, { "goes": null }] },
            "state": 1,
            "appearance": [{}, { "title": "On", "fontStyle": "boldItalic" }]
        });
        let instance: ActionInstance = serde_json::from_value(stored.clone()).unwrap();
        let Action::Plugin(plugin) = &instance.action else {
            panic!("plugin action expected");
        };
        assert_eq!(plugin.plugin, "com.example.plugin");
        assert_eq!(
            instance.appearance[1].font_style,
            Some(FontStyle::BoldItalic)
        );
        assert_eq!(serde_json::to_value(&instance).unwrap(), stored);
    }

    #[test]
    fn defaults_are_left_out() {
        let instance = ActionInstance::new(Action::Core(CoreAction::NextPage));
        let value = serde_json::to_value(&instance).unwrap();
        assert_eq!(
            value,
            json!({
                "id": instance.id.to_string(),
                "action": "io.github.zhappysnake.superdeck.next-page",
                "plugin": "io.github.zhappysnake.superdeck",
            })
        );
    }

    #[test]
    fn core_actions_round_trip() {
        let step = ActionInstance::new(Action::Core(CoreAction::Delay { milliseconds: 250 }));
        let mut folder_keys = Keys::new();
        folder_keys.insert(
            Position::new(1, 0),
            ActionInstance::new(Action::Core(CoreAction::Sleep)),
        );
        let actions = [
            CoreAction::Folder { keys: folder_keys },
            CoreAction::Folder { keys: Keys::new() },
            CoreAction::SwitchProfile {
                profile: ProfileId::new(),
                page: 2,
            },
            CoreAction::NextPage,
            CoreAction::PreviousPage,
            CoreAction::GoToPage { page: 3 },
            CoreAction::PageIndicator,
            CoreAction::MultiAction {
                steps: vec![step.clone()],
            },
            CoreAction::MultiActionSwitch {
                steps: [vec![step], Vec::new()],
            },
            CoreAction::Delay { milliseconds: 0 },
            CoreAction::Brightness {
                mode: BrightnessMode::Increase,
                value: 10,
            },
            CoreAction::Sleep,
        ];
        for action in actions {
            let instance = ActionInstance::new(Action::Core(action));
            let value = serde_json::to_value(&instance).unwrap();
            let back: ActionInstance = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(back, instance, "{value}");
        }
    }

    #[test]
    fn bad_instances_are_rejected() {
        let base = || {
            json!({
                "id": "5c0a7e4e-9a42-4e0f-b3c5-0d6c8e2f1b7a",
                "action": "io.github.zhappysnake.superdeck.go-to-page",
                "plugin": "io.github.zhappysnake.superdeck",
                "settings": { "page": 1 }
            })
        };
        assert!(serde_json::from_value::<ActionInstance>(base()).is_ok());
        let cases: [(&str, Value); 6] = [
            ("unknown field", json!({ "extra": true })),
            (
                "unknown core action",
                json!({ "action": "io.github.zhappysnake.superdeck.dance" }),
            ),
            (
                "core action without prefix",
                json!({ "action": "go-to-page" }),
            ),
            (
                "unknown setting",
                json!({ "settings": { "page": 1, "extra": 2 } }),
            ),
            ("missing setting", json!({ "settings": {} })),
            ("settings not an object", json!({ "settings": [1] })),
        ];
        for (what, patch) in cases {
            let mut value = base();
            for (key, field) in patch.as_object().unwrap() {
                value[key] = field.clone();
            }
            assert!(
                serde_json::from_value::<ActionInstance>(value).is_err(),
                "{what}"
            );
        }
    }

    #[test]
    fn instances_walks_folders_and_steps() {
        let leaf = || ActionInstance::new(Action::Core(CoreAction::Sleep));
        let mut inner = Keys::new();
        inner.insert(Position::new(1, 0), leaf());
        let multi = ActionInstance::new(Action::Core(CoreAction::MultiAction {
            steps: vec![leaf(), leaf()],
        }));
        inner.insert(Position::new(2, 0), multi);
        let folder = ActionInstance::new(Action::Core(CoreAction::Folder { keys: inner }));
        let mut profile = Profile::new(
            "Test",
            Layout {
                columns: 5,
                rows: 3,
            },
        );
        profile.pages[0].keys.insert(Position::new(0, 0), folder);
        profile.pages[0].keys.insert(Position::new(1, 0), leaf());
        // Folder, its two keys, the Multi Action's two steps, then the
        // second top-level key.
        assert_eq!(profile.instances().count(), 6);
        let names: Vec<_> = profile
            .instances()
            .map(|i| i.core().unwrap().name())
            .collect();
        assert_eq!(
            names,
            ["folder", "sleep", "multi-action", "sleep", "sleep", "sleep"]
        );

        let ids: Vec<_> = profile.instances().map(|i| i.id).collect();
        for id in ids {
            assert_eq!(profile.instance(id).unwrap().id, id);
            profile.instance_mut(id).unwrap().state = 1;
        }
        assert!(profile.instances().all(|i| i.state == 1));
        assert!(profile.instance(InstanceId::new()).is_none());
    }
}
