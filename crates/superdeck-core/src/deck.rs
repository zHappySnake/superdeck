// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a profile on a device: what each key shows, and what happens when
//! keys are pressed.
//!
//! A [`Deck`] does no I/O and reads no clock. The caller passes in key
//! presses and the time, and carries out the [`Effect`]s that come back:
//! drawing keys, setting the brightness, sending events to plugins,
//! switching profiles, and saving. That keeps every behavior testable
//! without hardware.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::ids::{InstanceId, ProfileId};
use crate::model::{
    Action, ActionInstance, BrightnessMode, Color, CoreAction, FontStyle, ImagePath, Keys,
    Position, Profile, Resources, TitleAlignment,
};

/// Title size of keys the user hasn't customized.
pub const DEFAULT_FONT_SIZE: f64 = 12.0;

/// Title size of the page indicator, which has no icon.
const PAGE_INDICATOR_FONT_SIZE: f64 = 20.0;

/// The back key of an open folder.
const BACK_KEY: Position = Position::new(0, 0);

/// How long `showAlert` and `showOk` show on a key.
pub const FLASH: Duration = Duration::from_millis(1500);

/// Device settings a deck needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckSettings {
    /// Backlight, 0 to 100.
    pub brightness: u8,
    /// Turn the screen off after this long without a key press.
    pub sleep_after: Option<Duration>,
}

/// Something the caller has to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Show `face` on the key at `position`, or turn it black for `None`.
    Draw {
        /// The key.
        position: Position,
        /// What to show.
        face: Option<KeyFace>,
    },
    /// Set the backlight. `save` is set when the user changed it, so it
    /// belongs in the device settings.
    Brightness {
        /// Percentage.
        percent: u8,
        /// Whether to store it as the device's brightness.
        save: bool,
    },
    /// Turn the screen off. The next key press wakes it, and does nothing
    /// else.
    Sleep,
    /// The screen is back on; draws and the brightness follow.
    Wake,
    /// Tell a plugin something.
    Plugin(PluginEvent),
    /// Show another profile.
    SwitchProfile {
        /// The profile.
        profile: ProfileId,
        /// Its page to show.
        page: usize,
    },
    /// The profile changed (a state or settings); save it soon.
    ProfileChanged,
}

/// An event for the plugin that owns an action instance.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginEvent {
    /// What happened.
    pub kind: PluginEventKind,
    /// The instance; the event's `context`.
    pub instance: InstanceId,
    /// Plugin UUID.
    pub plugin: String,
    /// Action UUID.
    pub action: String,
    /// The instance's settings.
    pub settings: Map<String, Value>,
    /// The instance's resources.
    pub resources: Resources,
    /// The instance's current state.
    pub state: u8,
    /// How many states the action has, from its manifest: 1 or 2.
    pub states: u8,
    /// The key, or `None` for a Multi Action step.
    pub position: Option<Position>,
}

/// Kinds of [`PluginEvent`], named after the SDK events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEventKind {
    /// `keyDown`.
    KeyDown,
    /// `keyUp`.
    KeyUp,
    /// `willAppear`: the key became visible.
    WillAppear,
    /// `willDisappear`: the key is no longer visible.
    WillDisappear,
    /// `didReceiveSettings`, built by [`Deck::describe`].
    DidReceiveSettings,
    /// `didReceiveResources`, built by [`Deck::describe`].
    DidReceiveResources,
}

/// What a key shows: an image and a title, with every title setting
/// resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyFace {
    /// The image, if any.
    pub image: Option<FaceImage>,
    /// Title text; may be empty.
    pub title: String,
    /// Whether the title is drawn.
    pub show_title: bool,
    /// Vertical position of the title.
    pub title_alignment: TitleAlignment,
    /// Font family; the default font if `None`.
    pub font_family: Option<String>,
    /// Pixels on a 72-pixel key.
    pub font_size: f64,
    /// Font style.
    pub font_style: FontStyle,
    /// Underlined title.
    pub font_underline: bool,
    /// Title color.
    pub title_color: Color,
}

impl Default for KeyFace {
    fn default() -> Self {
        Self {
            image: None,
            title: String::new(),
            show_title: true,
            title_alignment: TitleAlignment::Bottom,
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            font_style: FontStyle::Regular,
            font_underline: false,
            title_color: Color([255, 255, 255]),
        }
    }
}

/// Where a key's image comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FaceImage {
    /// A custom image in the profile directory.
    Profile(ImagePath),
    /// An image a plugin set, as a `data:` URL.
    DataUrl(String),
    /// One of SuperDeck's icons.
    Builtin(Icon),
    /// An image file of a plugin, named in its manifest.
    File(PathBuf),
}

/// SuperDeck's own icons, for the core actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "named after the actions")]
pub enum Icon {
    Back,
    Folder,
    SwitchProfile,
    NextPage,
    PreviousPage,
    GoToPage,
    MultiAction,
    MultiActionSwitch,
    Brightness,
    Sleep,
    /// Flashed by `showAlert`.
    Alert,
    /// Flashed by `showOk`.
    Ok,
}

impl Icon {
    /// The icon as SVG.
    #[must_use]
    pub fn svg(self) -> &'static [u8] {
        match self {
            Icon::Back => include_bytes!("../icons/back.svg"),
            Icon::Folder => include_bytes!("../icons/folder.svg"),
            Icon::SwitchProfile => include_bytes!("../icons/switch-profile.svg"),
            Icon::NextPage => include_bytes!("../icons/next-page.svg"),
            Icon::PreviousPage => include_bytes!("../icons/previous-page.svg"),
            Icon::GoToPage => include_bytes!("../icons/go-to-page.svg"),
            Icon::MultiAction => include_bytes!("../icons/multi-action.svg"),
            Icon::MultiActionSwitch => include_bytes!("../icons/multi-action-switch.svg"),
            Icon::Brightness => include_bytes!("../icons/brightness.svg"),
            Icon::Sleep => include_bytes!("../icons/sleep.svg"),
            Icon::Alert => include_bytes!("../icons/alert.svg"),
            Icon::Ok => include_bytes!("../icons/ok.svg"),
        }
    }

    fn for_action(action: &CoreAction) -> Option<Self> {
        Some(match action {
            CoreAction::Folder { .. } => Icon::Folder,
            CoreAction::SwitchProfile { .. } => Icon::SwitchProfile,
            CoreAction::NextPage => Icon::NextPage,
            CoreAction::PreviousPage => Icon::PreviousPage,
            CoreAction::GoToPage { .. } => Icon::GoToPage,
            CoreAction::MultiAction { .. } => Icon::MultiAction,
            CoreAction::MultiActionSwitch { .. } => Icon::MultiActionSwitch,
            CoreAction::Brightness { .. } => Icon::Brightness,
            CoreAction::Sleep => Icon::Sleep,
            CoreAction::PageIndicator | CoreAction::Delay { .. } => return None,
        })
    }
}

/// What a plugin's manifest says about one of its actions.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionDefaults {
    /// How each state looks: one or two.
    pub states: Vec<StateDefaults>,
    /// Switch a two-state action to its other state when its key is
    /// released; the manifest's `DisableAutomaticStates` turns this off.
    pub automatic_states: bool,
}

impl Default for ActionDefaults {
    fn default() -> Self {
        Self {
            states: vec![StateDefaults::default()],
            automatic_states: true,
        }
    }
}

/// How a state looks until the plugin or the user changes it. `None`
/// leaves SuperDeck's default.
#[derive(Debug, Clone, Default, PartialEq)]
#[expect(missing_docs, reason = "as in KeyFace")]
pub struct StateDefaults {
    pub image: Option<FaceImage>,
    pub title: Option<String>,
    pub show_title: Option<bool>,
    pub title_alignment: Option<TitleAlignment>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_style: Option<FontStyle>,
    pub font_underline: Option<bool>,
    pub title_color: Option<Color>,
}

/// The installed plugins' actions, by plugin UUID and action UUID.
pub type Catalog = HashMap<(String, String), ActionDefaults>;

/// What a plugin set on an instance at run time, for one state.
#[derive(Debug, Clone, Default)]
struct PluginFace {
    title: Option<String>,
    image: Option<String>,
}

/// What a held key does when it's released.
#[derive(Debug, Clone, Copy)]
enum Held {
    /// Send `keyUp` to this instance.
    Instance(InstanceId),
    /// Nothing: the press woke the screen, or was handled on key down.
    Nothing,
}

/// A Multi Action step waiting for its time.
#[derive(Debug)]
struct Scheduled {
    due: Instant,
    step: ActionInstance,
}

/// A profile running on a device.
#[derive(Debug)]
pub struct Deck {
    profile: Profile,
    page: usize,
    /// Positions of the open folders, outermost first.
    folders: Vec<Position>,
    held: HashMap<Position, Held>,
    plugin_faces: HashMap<(InstanceId, u8), PluginFace>,
    brightness: u8,
    sleep_after: Option<Duration>,
    sleeping: bool,
    last_activity: Instant,
    /// Steps in the order they run; dues never decrease.
    queue: Vec<Scheduled>,
    catalog: Arc<Catalog>,
    /// Instances showing an alert or a check mark, until when.
    flashes: HashMap<InstanceId, (Icon, Instant)>,
}

impl Deck {
    /// Runs `profile`, showing `page` (or the first page if it doesn't
    /// exist). Call [`start`](Self::start) next.
    #[must_use]
    pub fn new(profile: Profile, settings: DeckSettings, page: usize, now: Instant) -> Self {
        let page = if page < profile.pages.len() { page } else { 0 };
        Self {
            profile,
            page,
            folders: Vec::new(),
            held: HashMap::new(),
            plugin_faces: HashMap::new(),
            brightness: settings.brightness.min(100),
            sleep_after: settings.sleep_after,
            sleeping: false,
            last_activity: now,
            queue: Vec::new(),
            catalog: Arc::new(Catalog::new()),
            flashes: HashMap::new(),
        }
    }

    /// Uses the manifests' defaults for plugin actions: their images and
    /// titles, how many states they have, and whether those switch by
    /// themselves.
    #[must_use]
    pub fn with_catalog(mut self, catalog: Arc<Catalog>) -> Self {
        self.catalog = catalog;
        self
    }

    fn defaults(&self, plugin: &crate::model::PluginAction) -> Option<&ActionDefaults> {
        self.catalog
            .get(&(plugin.plugin.clone(), plugin.action.clone()))
    }

    /// The profile, with any changes made while running.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Index of the page showing.
    #[must_use]
    pub fn page(&self) -> usize {
        self.page
    }

    /// Whether a folder is open.
    #[must_use]
    pub fn in_folder(&self) -> bool {
        !self.folders.is_empty()
    }

    /// Whether the screen is off.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    /// The effects that bring a device up: brightness, every key, and
    /// `willAppear` for the visible plugin actions.
    pub fn start(&mut self) -> Vec<Effect> {
        let mut effects = vec![Effect::Brightness {
            percent: self.brightness,
            save: false,
        }];
        self.appear(&mut effects, PluginEventKind::WillAppear);
        self.draw_all(&mut effects);
        effects
    }

    /// `willDisappear` for the visible plugin actions, before switching to
    /// another profile or stopping.
    pub fn stop(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.appear(&mut effects, PluginEventKind::WillDisappear);
        effects
    }

    /// A key was pressed.
    pub fn key_down(&mut self, position: Position, now: Instant) -> Vec<Effect> {
        self.last_activity = now;
        let mut effects = Vec::new();
        if self.sleeping {
            self.held.insert(position, Held::Nothing);
            self.wake(&mut effects);
            return effects;
        }
        if self.in_folder() && position == BACK_KEY {
            self.held.insert(position, Held::Nothing);
            self.navigate(&mut effects, |deck| {
                deck.folders.pop();
            });
            return effects;
        }
        let Some(instance) = self.grid().get(&position) else {
            return effects;
        };
        let id = instance.id;
        let held = match &instance.action {
            Action::Plugin(_) => {
                effects.push(self.plugin_event(PluginEventKind::KeyDown, instance, Some(position)));
                Held::Instance(id)
            }
            Action::Core(core) => {
                let core = core.clone();
                self.run_core(&mut effects, id, Some(position), &core, now);
                Held::Nothing
            }
        };
        self.held.insert(position, held);
        effects.extend(self.poll(now));
        effects
    }

    /// A key was released.
    pub fn key_up(&mut self, position: Position, now: Instant) -> Vec<Effect> {
        self.last_activity = now;
        let mut effects = Vec::new();
        // The instance pressed, even if the page has changed since.
        if let Some(Held::Instance(id)) = self.held.remove(&position)
            && let Some(instance) = self.profile.instance(id)
        {
            let event = self.plugin_event(PluginEventKind::KeyUp, instance, Some(position));
            let toggles = match (&event, &instance.action) {
                (Effect::Plugin(event), Action::Plugin(plugin)) => {
                    event.states == 2
                        && self
                            .defaults(plugin)
                            .is_none_or(|defaults| defaults.automatic_states)
                }
                _ => false,
            };
            effects.push(event);
            if toggles {
                let state = 1 - instance.state.min(1);
                effects.extend(self.set_state(id, state));
            }
        }
        effects
    }

    /// Runs what's due by `now`: Multi Action steps after a delay, and
    /// going to sleep.
    pub fn poll(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let ended: Vec<InstanceId> = self
            .flashes
            .iter()
            .filter(|(_, (_, until))| *until <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in ended {
            self.flashes.remove(&id);
            self.redraw(&mut effects, id);
        }
        let due = self.queue.partition_point(|scheduled| scheduled.due <= now);
        for scheduled in self.queue.drain(..due).collect::<Vec<_>>() {
            self.run_step(&mut effects, &scheduled.step, now);
        }
        if let Some(sleep_at) = self.sleep_at()
            && now >= sleep_at
        {
            self.sleeping = true;
            effects.push(Effect::Sleep);
        }
        effects
    }

    /// When [`poll`](Self::poll) has something to do next, if ever.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        let step = self.queue.first().map(|scheduled| scheduled.due);
        let flash = self.flashes.values().map(|(_, until)| *until).min();
        [step, flash, self.sleep_at()].into_iter().flatten().min()
    }

    fn sleep_at(&self) -> Option<Instant> {
        if self.sleeping {
            return None;
        }
        self.sleep_after.map(|after| self.last_activity + after)
    }

    /// A plugin set a title (`setTitle`), for one state or both. `None`
    /// clears it.
    pub fn set_title(
        &mut self,
        instance: InstanceId,
        state: Option<u8>,
        title: Option<&str>,
    ) -> Vec<Effect> {
        self.set_plugin_face(instance, state, |face| {
            face.title = title.map(str::to_owned);
        })
    }

    /// A plugin set an image as a `data:` URL (`setImage`), for one state or
    /// both. `None` clears it.
    pub fn set_image(
        &mut self,
        instance: InstanceId,
        state: Option<u8>,
        image: Option<&str>,
    ) -> Vec<Effect> {
        self.set_plugin_face(instance, state, |face| {
            face.image = image.map(str::to_owned);
        })
    }

    /// A plugin changed an instance's state (`setState`).
    pub fn set_state(&mut self, instance: InstanceId, state: u8) -> Vec<Effect> {
        let Some(found) = self.profile.instance_mut(instance) else {
            return Vec::new();
        };
        if state > 1 || found.state == state {
            return Vec::new();
        }
        found.state = state;
        let mut effects = vec![Effect::ProfileChanged];
        self.redraw(&mut effects, instance);
        effects
    }

    /// A plugin saved an instance's settings (`setSettings`).
    pub fn set_settings(
        &mut self,
        instance: InstanceId,
        settings: Map<String, Value>,
    ) -> Vec<Effect> {
        match self
            .profile
            .instance_mut(instance)
            .map(|found| &mut found.action)
        {
            Some(Action::Plugin(plugin)) if plugin.settings != settings => {
                plugin.settings = settings;
                vec![Effect::ProfileChanged]
            }
            _ => Vec::new(),
        }
    }

    /// A plugin saved an instance's resources (`setResources`).
    pub fn set_resources(&mut self, instance: InstanceId, resources: Resources) -> Vec<Effect> {
        match self
            .profile
            .instance_mut(instance)
            .map(|found| &mut found.action)
        {
            Some(Action::Plugin(plugin)) if plugin.resources != resources => {
                plugin.resources = resources;
                vec![Effect::ProfileChanged]
            }
            _ => Vec::new(),
        }
    }

    /// A plugin asked to show a warning on an instance's key for a moment
    /// (`showAlert`).
    pub fn show_alert(&mut self, instance: InstanceId, now: Instant) -> Vec<Effect> {
        self.flash(instance, Icon::Alert, now)
    }

    /// A plugin asked to show a check mark on an instance's key for a
    /// moment (`showOk`).
    pub fn show_ok(&mut self, instance: InstanceId, now: Instant) -> Vec<Effect> {
        self.flash(instance, Icon::Ok, now)
    }

    fn flash(&mut self, instance: InstanceId, icon: Icon, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.sleeping || self.profile.instance(instance).is_none() {
            return effects;
        }
        self.flashes.insert(instance, (icon, now + FLASH));
        self.redraw(&mut effects, instance);
        effects
    }

    /// `willAppear` for the plugin's actions that are showing, for a plugin
    /// that just connected.
    #[must_use]
    pub fn announce(&self, plugin: &str) -> Vec<Effect> {
        self.grid()
            .iter()
            .filter(|(position, _)| !(self.in_folder() && **position == BACK_KEY))
            .filter(|(_, instance)| {
                matches!(&instance.action, Action::Plugin(action) if action.plugin == plugin)
            })
            .map(|(&position, instance)| {
                self.plugin_event(PluginEventKind::WillAppear, instance, Some(position))
            })
            .collect()
    }

    /// Whether an instance in the profile is an action of `plugin`.
    #[must_use]
    pub fn owns(&self, plugin: &str, instance: InstanceId) -> bool {
        matches!(
            self.profile.instance(instance).map(|found| &found.action),
            Some(Action::Plugin(action)) if action.plugin == plugin
        )
    }

    /// An event about any plugin instance in the profile, such as the
    /// answer to `getSettings`: its key if it's on one, whether or not it's
    /// showing, or no key for a Multi Action step.
    #[must_use]
    pub fn describe(&self, kind: PluginEventKind, instance: InstanceId) -> Option<PluginEvent> {
        fn find(keys: &Keys, id: InstanceId) -> Option<(Option<Position>, &ActionInstance)> {
            for (&position, instance) in keys {
                if instance.id == id {
                    return Some((Some(position), instance));
                }
                if let Some(CoreAction::Folder { keys }) = instance.core()
                    && let Some(found) = find(keys, id)
                {
                    return Some(found);
                }
                if let Some(step) = instance.children().find(|child| child.id == id) {
                    return Some((None, step));
                }
            }
            None
        }
        let (position, instance) = self
            .profile
            .pages
            .iter()
            .find_map(|page| find(&page.keys, instance))?;
        self.event(kind, instance, position)
    }

    fn set_plugin_face(
        &mut self,
        instance: InstanceId,
        state: Option<u8>,
        change: impl Fn(&mut PluginFace),
    ) -> Vec<Effect> {
        if self.profile.instance(instance).is_none() {
            return Vec::new();
        }
        let states = match state {
            Some(state) if state <= 1 => state..=state,
            Some(_) => return Vec::new(),
            None => 0..=1,
        };
        for state in states {
            change(self.plugin_faces.entry((instance, state)).or_default());
        }
        let mut effects = Vec::new();
        self.redraw(&mut effects, instance);
        effects
    }

    /// The keys showing: the page's, or the innermost open folder's.
    fn grid(&self) -> &Keys {
        let mut keys = &self.profile.pages[self.page].keys;
        for position in &self.folders {
            match keys.get(position).and_then(ActionInstance::core) {
                Some(CoreAction::Folder { keys: inner }) => keys = inner,
                _ => break,
            }
        }
        keys
    }

    /// What the key at `position` shows.
    #[must_use]
    pub fn face(&self, position: Position) -> Option<KeyFace> {
        if self.in_folder() && position == BACK_KEY {
            return Some(KeyFace {
                image: Some(FaceImage::Builtin(Icon::Back)),
                ..KeyFace::default()
            });
        }
        self.grid()
            .get(&position)
            .map(|instance| self.face_of(instance))
    }

    fn face_of(&self, instance: &ActionInstance) -> KeyFace {
        if let Some((icon, _)) = self.flashes.get(&instance.id) {
            return KeyFace {
                image: Some(FaceImage::Builtin(*icon)),
                show_title: false,
                ..KeyFace::default()
            };
        }
        let mut face = KeyFace::default();
        match &instance.action {
            Action::Core(CoreAction::PageIndicator) => {
                face.title = format!("{}/{}", self.page + 1, self.profile.pages.len());
                face.title_alignment = TitleAlignment::Middle;
                face.font_size = PAGE_INDICATOR_FONT_SIZE;
            }
            Action::Core(core) => face.image = Icon::for_action(core).map(FaceImage::Builtin),
            Action::Plugin(action) => {
                if let Some(defaults) = self
                    .defaults(action)
                    .and_then(|defaults| defaults.states.get(usize::from(instance.state)))
                {
                    apply_defaults(&mut face, defaults);
                }
                if let Some(plugin) = self.plugin_faces.get(&(instance.id, instance.state)) {
                    if let Some(image) = &plugin.image {
                        face.image = Some(FaceImage::DataUrl(image.clone()));
                    }
                    if let Some(title) = &plugin.title {
                        face.title.clone_from(title);
                    }
                }
            }
        }
        // The user's choices win, field by field.
        if let Some(user) = instance.appearance.get(usize::from(instance.state)) {
            if let Some(image) = &user.image {
                face.image = Some(FaceImage::Profile(image.clone()));
            }
            if let Some(title) = &user.title {
                face.title.clone_from(title);
            }
            if let Some(show) = user.show_title {
                face.show_title = show;
            }
            if let Some(alignment) = user.title_alignment {
                face.title_alignment = alignment;
            }
            if let Some(family) = &user.font_family {
                face.font_family = Some(family.clone());
            }
            if let Some(size) = user.font_size {
                face.font_size = size.get();
            }
            if let Some(style) = user.font_style {
                face.font_style = style;
            }
            if let Some(underline) = user.font_underline {
                face.font_underline = underline;
            }
            if let Some(color) = user.title_color {
                face.title_color = color;
            }
        }
        face
    }

    fn positions(&self) -> impl Iterator<Item = Position> + use<> {
        let layout = self.profile.layout;
        (0..layout.rows)
            .flat_map(move |row| (0..layout.columns).map(move |column| Position::new(column, row)))
    }

    fn draw_all(&self, effects: &mut Vec<Effect>) {
        for position in self.positions() {
            effects.push(Effect::Draw {
                position,
                face: self.face(position),
            });
        }
    }

    /// Redraws an instance if it's showing.
    fn redraw(&self, effects: &mut Vec<Effect>, instance: InstanceId) {
        if self.sleeping {
            return;
        }
        let shown = self.grid().iter().find(|(position, shown)| {
            shown.id == instance && !(self.in_folder() && **position == BACK_KEY)
        });
        if let Some((&position, _)) = shown {
            effects.push(Effect::Draw {
                position,
                face: self.face(position),
            });
        }
    }

    /// Sends `kind` to every visible plugin action.
    fn appear(&self, effects: &mut Vec<Effect>, kind: PluginEventKind) {
        for (&position, instance) in self.grid() {
            if matches!(instance.action, Action::Plugin(_)) {
                effects.push(self.plugin_event(kind, instance, Some(position)));
            }
        }
    }

    /// Changes what's showing: plugin actions disappear, `change` runs, and
    /// everything is drawn again.
    fn navigate(&mut self, effects: &mut Vec<Effect>, change: impl FnOnce(&mut Self)) {
        self.appear(effects, PluginEventKind::WillDisappear);
        change(self);
        self.appear(effects, PluginEventKind::WillAppear);
        if !self.sleeping {
            self.draw_all(effects);
        }
    }

    fn wake(&mut self, effects: &mut Vec<Effect>) {
        self.sleeping = false;
        effects.push(Effect::Wake);
        effects.push(Effect::Brightness {
            percent: self.brightness,
            save: false,
        });
        self.draw_all(effects);
    }

    fn go_to_page(&mut self, effects: &mut Vec<Effect>, page: usize) {
        if page < self.profile.pages.len() {
            self.navigate(effects, |deck| {
                deck.page = page;
                deck.folders.clear();
            });
        }
    }

    fn run_core(
        &mut self,
        effects: &mut Vec<Effect>,
        id: InstanceId,
        position: Option<Position>,
        core: &CoreAction,
        now: Instant,
    ) {
        let pages = self.profile.pages.len();
        match core {
            CoreAction::Folder { .. } => {
                if let Some(position) = position {
                    self.navigate(effects, |deck| deck.folders.push(position));
                }
            }
            CoreAction::NextPage => self.go_to_page(effects, (self.page + 1) % pages),
            CoreAction::PreviousPage => self.go_to_page(effects, (self.page + pages - 1) % pages),
            CoreAction::GoToPage { page } => self.go_to_page(effects, *page),
            CoreAction::PageIndicator | CoreAction::Delay { .. } => {}
            CoreAction::SwitchProfile { profile, page } => effects.push(Effect::SwitchProfile {
                profile: *profile,
                page: *page,
            }),
            CoreAction::MultiAction { steps } => self.schedule(steps, now),
            CoreAction::MultiActionSwitch { steps } => {
                let Some(instance) = self.profile.instance_mut(id) else {
                    return;
                };
                let state = usize::from(instance.state.min(1));
                instance.state = 1 - instance.state.min(1);
                self.schedule(&steps[state], now);
                effects.push(Effect::ProfileChanged);
                self.redraw(effects, id);
            }
            CoreAction::Brightness { mode, value } => {
                self.brightness = match mode {
                    BrightnessMode::Set => *value,
                    BrightnessMode::Increase => self.brightness.saturating_add(*value),
                    BrightnessMode::Decrease => self.brightness.saturating_sub(*value),
                }
                .min(100);
                effects.push(Effect::Brightness {
                    percent: self.brightness,
                    save: true,
                });
            }
            CoreAction::Sleep => {
                self.sleeping = true;
                effects.push(Effect::Sleep);
            }
        }
    }

    /// Queues Multi Action steps from `now`, spaced by their delays.
    fn schedule(&mut self, steps: &[ActionInstance], now: Instant) {
        let mut due = now;
        for step in steps {
            if let Some(CoreAction::Delay { milliseconds }) = step.core() {
                due += Duration::from_millis(u64::from(*milliseconds));
            } else {
                // Keep the queue in order of due time; equal times keep the
                // order they were queued in.
                let at = self.queue.partition_point(|scheduled| scheduled.due <= due);
                self.queue.insert(
                    at,
                    Scheduled {
                        due,
                        step: step.clone(),
                    },
                );
            }
        }
    }

    /// An event for the plugin that owns `instance`, a plugin action.
    fn plugin_event(
        &self,
        kind: PluginEventKind,
        instance: &ActionInstance,
        position: Option<Position>,
    ) -> Effect {
        Effect::Plugin(
            self.event(kind, instance, position)
                .expect("only plugin actions get plugin events"),
        )
    }

    fn event(
        &self,
        kind: PluginEventKind,
        instance: &ActionInstance,
        position: Option<Position>,
    ) -> Option<PluginEvent> {
        let Action::Plugin(plugin) = &instance.action else {
            return None;
        };
        let states = self
            .defaults(plugin)
            .map_or(1, |defaults| defaults.states.len().clamp(1, 2));
        Some(PluginEvent {
            kind,
            instance: instance.id,
            plugin: plugin.plugin.clone(),
            action: plugin.action.clone(),
            settings: plugin.settings.clone(),
            resources: plugin.resources.clone(),
            state: instance.state,
            states: u8::try_from(states).expect("1 or 2"),
            position,
        })
    }

    fn run_step(&mut self, effects: &mut Vec<Effect>, step: &ActionInstance, now: Instant) {
        match &step.action {
            Action::Plugin(_) => {
                effects.push(self.plugin_event(PluginEventKind::KeyDown, step, None));
                effects.push(self.plugin_event(PluginEventKind::KeyUp, step, None));
            }
            Action::Core(core) => self.run_core(effects, step.id, None, core, now),
        }
    }
}

/// A manifest's defaults for a state, over SuperDeck's.
fn apply_defaults(face: &mut KeyFace, defaults: &StateDefaults) {
    if let Some(image) = &defaults.image {
        face.image = Some(image.clone());
    }
    if let Some(title) = &defaults.title {
        face.title.clone_from(title);
    }
    if let Some(show) = defaults.show_title {
        face.show_title = show;
    }
    if let Some(alignment) = defaults.title_alignment {
        face.title_alignment = alignment;
    }
    if let Some(family) = &defaults.font_family {
        face.font_family = Some(family.clone());
    }
    if let Some(size) = defaults.font_size {
        face.font_size = size;
    }
    if let Some(style) = defaults.font_style {
        face.font_style = style;
    }
    if let Some(underline) = defaults.font_underline {
        face.font_underline = underline;
    }
    if let Some(color) = defaults.title_color {
        face.title_color = color;
    }
}
