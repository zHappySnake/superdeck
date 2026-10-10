// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's state, without I/O: who is connected, which contexts each
//! plugin owns, and where every message goes. The server feeds it
//! connections and messages and carries out its [`Output`]s.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
#[cfg(test)]
use serde_json::Value;

use crate::global_settings::GlobalSettings;
use crate::images;
use crate::info::{AppInfo, registration_info};
use crate::log::{PluginLog, Source};
use crate::manifest::Plugin;
use crate::message::{
    ActionPayload, Command, Device, Event, GlobalSettingsPayload, InspectorEvent, Instance,
    ParseError, Registration, Resources, Settings,
};

/// Identifies a WebSocket connection.
pub(crate) type ConnId = u64;

/// The `-registerEvent` of plugins.
pub(crate) const REGISTER_PLUGIN: &str = "registerPlugin";

/// The `registerEvent` of property inspectors.
pub(crate) const REGISTER_INSPECTOR: &str = "registerPropertyInspector";

/// Who sent a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    /// The plugin.
    Plugin,
    /// The instance's property inspector.
    PropertyInspector,
}

/// Something a plugin or property inspector asked the app to do, or news
/// about a plugin. Contexts are always ones the plugin owns.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Request {
    /// The plugin's process started.
    Started {
        /// Plugin UUID.
        plugin: String,
    },
    /// The plugin's process exited, or couldn't start; it's started again
    /// after a delay.
    Stopped {
        /// Plugin UUID.
        plugin: String,
        /// What happened, for people.
        reason: String,
    },
    /// The plugin connected. Send it `willAppear` for its instances that
    /// are showing.
    Registered {
        /// Plugin UUID.
        plugin: String,
    },
    /// The plugin's connection closed.
    Disconnected {
        /// Plugin UUID.
        plugin: String,
    },
    /// Save an instance's settings, then send `didReceiveSettings` to the
    /// other side: the property inspector if the plugin saved them, the
    /// plugin if the inspector did.
    SetSettings {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// The new settings.
        settings: Settings,
        /// Who saved them.
        from: Sender,
    },
    /// Answer with `didReceiveSettings`, with this `id`, to whoever asked.
    GetSettings {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// To echo.
        id: Option<String>,
        /// Who asked.
        from: Sender,
    },
    /// Save an instance's resources, then tell the other side, as for
    /// settings.
    SetResources {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// The new resources.
        resources: Resources,
        /// Who saved them.
        from: Sender,
    },
    /// Answer with `didReceiveResources`, with this `id`, to whoever asked.
    GetResources {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// To echo.
        id: Option<String>,
        /// Who asked.
        from: Sender,
    },
    /// Show a title, for one state or both; `None` resets it.
    SetTitle {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// The state, or both.
        state: Option<u8>,
        /// The title.
        title: Option<String>,
    },
    /// Show an image, for one state or both; `None` resets it.
    SetImage {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// The state, or both.
        state: Option<u8>,
        /// A `data:` URL.
        image: Option<String>,
    },
    /// Change an instance's state.
    SetState {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
        /// The state.
        state: u8,
    },
    /// Show a warning on the key for a moment.
    ShowAlert {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
    },
    /// Show a check mark on the key for a moment.
    ShowOk {
        /// Plugin UUID.
        plugin: String,
        /// The instance.
        context: String,
    },
    /// Switch a device to a profile the plugin ships, or back to the
    /// previous one for `None`.
    SwitchToProfile {
        /// Plugin UUID.
        plugin: String,
        /// Device ID.
        device: String,
        /// Profile name, as in the manifest.
        profile: Option<String>,
        /// Page to show.
        page: Option<usize>,
    },
    /// Open a URL in the default browser.
    OpenUrl {
        /// Plugin UUID.
        plugin: String,
        /// The URL, as given.
        url: String,
    },
}

/// What a property inspector needs to connect: the arguments of its
/// `connectElgatoStreamDeckSocket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorLaunch {
    /// `inPort`.
    pub port: u16,
    /// `inPropertyInspectorUUID`: the context.
    pub uuid: String,
    /// `inRegisterEvent`.
    pub register_event: String,
    /// `inInfo`, JSON.
    pub info: String,
    /// `inActionInfo`, JSON.
    pub action_info: String,
}

/// What a plugin needs to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLaunch {
    /// `-port`.
    pub port: u16,
    /// `-pluginUUID`: the start token.
    pub token: String,
    /// `-registerEvent`.
    pub register_event: String,
    /// `-info`, JSON.
    pub info: String,
}

impl PluginLaunch {
    /// The arguments after the script, as the SDK documents them.
    #[must_use]
    pub fn args(&self) -> [String; 8] {
        [
            "-port".to_owned(),
            self.port.to_string(),
            "-pluginUUID".to_owned(),
            self.token.clone(),
            "-registerEvent".to_owned(),
            self.register_event.clone(),
            "-info".to_owned(),
            self.info.clone(),
        ]
    }
}

/// Something the server has to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Output {
    /// Send a text message.
    Send(ConnId, String),
    /// Close a connection.
    Close(ConnId),
    /// Pass a request to the app.
    Request(Request),
}

/// A plugin the host knows.
#[derive(Debug)]
struct Slot {
    plugin: Arc<Plugin>,
    log: PluginLog,
    /// The start token that registers it now.
    token: Option<String>,
    conn: Option<ConnId>,
    /// Contexts it has been told about, so it may address them.
    contexts: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Peer {
    /// Connected, not registered yet.
    Pending,
    /// A plugin, by UUID.
    Plugin(String),
    /// A property inspector, by context.
    Inspector(String),
}

/// An open property inspector.
#[derive(Debug)]
struct Inspector {
    plugin: String,
    action: String,
    device: String,
    conn: Option<ConnId>,
}

/// See the module documentation.
#[derive(Debug)]
pub(crate) struct Router {
    port: u16,
    app: AppInfo,
    devices: Vec<Device>,
    plugins: BTreeMap<String, Slot>,
    /// Start token → plugin UUID.
    tokens: HashMap<String, String>,
    peers: HashMap<ConnId, Peer>,
    /// By context.
    inspectors: HashMap<String, Inspector>,
    global: GlobalSettings,
}

impl Router {
    pub fn new(
        port: u16,
        app: AppInfo,
        plugins: impl IntoIterator<Item = (Arc<Plugin>, PluginLog)>,
        global: GlobalSettings,
    ) -> Self {
        let plugins = plugins
            .into_iter()
            .map(|(plugin, log)| {
                let slot = Slot {
                    plugin: Arc::clone(&plugin),
                    log,
                    token: None,
                    conn: None,
                    contexts: HashSet::new(),
                };
                (plugin.uuid.clone(), slot)
            })
            .collect();
        Self {
            port,
            app,
            devices: Vec::new(),
            plugins,
            tokens: HashMap::new(),
            peers: HashMap::new(),
            inspectors: HashMap::new(),
            global,
        }
    }

    /// A plugin is about to start: a new token registers it, and any
    /// earlier connection is closed. `None` for unknown plugins.
    pub fn launch(&mut self, plugin: &str) -> Option<(PluginLaunch, Vec<Output>)> {
        let out = self.stopped(plugin);
        let slot = self.plugins.get_mut(plugin)?;
        let token = uuid::Uuid::new_v4().to_string();
        if let Some(old) = slot.token.replace(token.clone()) {
            self.tokens.remove(&old);
        }
        self.tokens.insert(token.clone(), plugin.to_owned());
        let info = registration_info(
            &self.app,
            &slot.plugin.uuid,
            &slot.plugin.manifest.version,
            &self.devices,
        );
        let launch = PluginLaunch {
            port: self.port,
            token,
            register_event: REGISTER_PLUGIN.to_owned(),
            info: info.to_string(),
        };
        Some((launch, out))
    }

    /// A plugin's process ended: its token no longer registers, and its
    /// connection is closed.
    pub fn stopped(&mut self, plugin: &str) -> Vec<Output> {
        let mut out = Vec::new();
        let Some(slot) = self.plugins.get_mut(plugin) else {
            return out;
        };
        if let Some(token) = slot.token.take() {
            self.tokens.remove(&token);
        }
        if let Some(conn) = slot.conn.take() {
            self.peers.remove(&conn);
            out.push(Output::Close(conn));
            out.push(Output::Request(Request::Disconnected {
                plugin: plugin.to_owned(),
            }));
        }
        out
    }

    /// Whether a plugin is connected.
    #[cfg(test)]
    pub fn is_connected(&self, plugin: &str) -> bool {
        self.plugins
            .get(plugin)
            .is_some_and(|slot| slot.conn.is_some())
    }

    /// A new connection, not registered yet.
    pub fn opened(&mut self, conn: ConnId) {
        self.peers.insert(conn, Peer::Pending);
    }

    /// A connection closed.
    pub fn closed(&mut self, conn: ConnId) -> Vec<Output> {
        let mut out = Vec::new();
        match self.peers.remove(&conn) {
            Some(Peer::Plugin(uuid)) => {
                if let Some(slot) = self.plugins.get_mut(&uuid)
                    && slot.conn == Some(conn)
                {
                    slot.conn = None;
                    slot.log.write(Source::Host, "disconnected");
                    out.push(Output::Request(Request::Disconnected { plugin: uuid }));
                }
            }
            Some(Peer::Inspector(context)) => {
                if let Some(inspector) = self.inspectors.get_mut(&context)
                    && inspector.conn == Some(conn)
                {
                    inspector.conn = None;
                    self.inspector_disappeared(&mut out, &context);
                }
            }
            Some(Peer::Pending) | None => {}
        }
        out
    }

    /// A text message arrived.
    pub fn received(&mut self, conn: ConnId, text: &str) -> Vec<Output> {
        let mut out = Vec::new();
        match self.peers.get(&conn).cloned() {
            None => {}
            Some(Peer::Pending) => self.register(&mut out, conn, text),
            Some(Peer::Plugin(uuid)) => self.plugin_command(&mut out, conn, &uuid, text),
            Some(Peer::Inspector(context)) => {
                self.inspector_command(&mut out, conn, &context, text);
            }
        }
        out
    }

    fn register(&mut self, out: &mut Vec<Output>, conn: ConnId, text: &str) {
        let registration = serde_json::from_str::<Registration>(text).ok();
        match registration
            .as_ref()
            .map(|r| (r.event.as_str(), r.uuid.as_str()))
        {
            Some((REGISTER_PLUGIN, token)) => {
                if let Some(uuid) = self.tokens.get(token).cloned()
                    && let Some(slot) = self.plugins.get_mut(&uuid)
                    && slot.conn.is_none()
                {
                    slot.conn = Some(conn);
                    slot.log.write(Source::Host, "registered");
                    self.peers.insert(conn, Peer::Plugin(uuid.clone()));
                    for device in &self.devices {
                        out.push(Output::Send(
                            conn,
                            to_json(&Event::DeviceDidConnect {
                                device: device.id.clone(),
                                device_info: device.info.clone(),
                            }),
                        ));
                    }
                    out.push(Output::Request(Request::Registered {
                        plugin: uuid.clone(),
                    }));
                    // An inspector that was open before the plugin
                    // (re)started.
                    let open: Vec<String> = self
                        .inspectors
                        .iter()
                        .filter(|(_, inspector)| {
                            inspector.plugin == uuid && inspector.conn.is_some()
                        })
                        .map(|(context, _)| context.clone())
                        .collect();
                    for context in open {
                        self.inspector_appeared(out, &context);
                    }
                    return;
                }
            }
            Some((REGISTER_INSPECTOR, context)) => {
                if let Some(inspector) = self.inspectors.get_mut(context)
                    && inspector.conn.is_none()
                {
                    inspector.conn = Some(conn);
                    let context = context.to_owned();
                    self.peers.insert(conn, Peer::Inspector(context.clone()));
                    self.inspector_appeared(out, &context);
                    return;
                }
            }
            _ => {}
        }
        self.peers.remove(&conn);
        out.push(Output::Close(conn));
    }

    #[expect(clippy::too_many_lines, reason = "one arm per command")]
    fn plugin_command(&mut self, out: &mut Vec<Output>, conn: ConnId, uuid: &str, text: &str) {
        let command = match Command::parse(text) {
            Ok(command) => command,
            Err(ParseError::Unknown(event)) if event.starts_with("register") => {
                self.write_log(
                    uuid,
                    Source::Host,
                    "registered a second time; disconnecting",
                );
                out.extend(self.stopped_connection(uuid));
                return;
            }
            Err(e) => {
                self.write_log(uuid, Source::Host, &format!("ignored a message: {e}"));
                return;
            }
        };
        let plugin = uuid.to_owned();
        // Commands addressing a context check it first.
        let owned = |router: &Self, context: &str| {
            let owns = router.plugins[uuid].contexts.contains(context);
            if !owns {
                router.write_log(
                    uuid,
                    Source::Host,
                    &format!("refused a message for context {context:?}, which isn't the plugin's"),
                );
            }
            owns
        };
        let request = match command {
            Command::SetSettings { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                Request::SetSettings {
                    plugin,
                    context,
                    settings: payload,
                    from: Sender::Plugin,
                }
            }
            Command::GetSettings { context, id } => {
                if !owned(self, &context) {
                    return;
                }
                Request::GetSettings {
                    plugin,
                    context,
                    id,
                    from: Sender::Plugin,
                }
            }
            Command::SetResources { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                Request::SetResources {
                    plugin,
                    context,
                    resources: payload,
                    from: Sender::Plugin,
                }
            }
            Command::GetResources { context, id } => {
                if !owned(self, &context) {
                    return;
                }
                Request::GetResources {
                    plugin,
                    context,
                    id,
                    from: Sender::Plugin,
                }
            }
            Command::SetGlobalSettings { payload } => {
                self.set_global(uuid, payload.clone());
                let event = to_json(&InspectorEvent::DidReceiveGlobalSettings {
                    payload: GlobalSettingsPayload { settings: payload },
                    id: None,
                });
                for inspector in self.inspectors.values() {
                    if inspector.plugin == uuid
                        && let Some(conn) = inspector.conn
                    {
                        out.push(Output::Send(conn, event.clone()));
                    }
                }
                return;
            }
            Command::GetGlobalSettings { id } => {
                let settings = self.get_global(uuid);
                out.push(Output::Send(
                    conn,
                    to_json(&Event::DidReceiveGlobalSettings {
                        payload: GlobalSettingsPayload { settings },
                        id,
                    }),
                ));
                return;
            }
            Command::SetTitle { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                Request::SetTitle {
                    plugin,
                    context,
                    state: payload.state,
                    title: payload.title,
                }
            }
            Command::SetImage { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                let image = match payload.image.filter(|image| !image.is_empty()) {
                    None => None,
                    Some(image) => match images::resolve(&self.plugins[uuid].plugin, &image) {
                        Ok(url) => Some(url),
                        Err(reason) => {
                            self.write_log(
                                uuid,
                                Source::Host,
                                &format!("setImage refused: {reason}"),
                            );
                            return;
                        }
                    },
                };
                Request::SetImage {
                    plugin,
                    context,
                    state: payload.state,
                    image,
                }
            }
            Command::SetState { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                Request::SetState {
                    plugin,
                    context,
                    state: payload.state,
                }
            }
            Command::ShowAlert { context } => {
                if !owned(self, &context) {
                    return;
                }
                Request::ShowAlert { plugin, context }
            }
            Command::ShowOk { context } => {
                if !owned(self, &context) {
                    return;
                }
                Request::ShowOk { plugin, context }
            }
            Command::SwitchToProfile { device, payload } => Request::SwitchToProfile {
                plugin,
                device,
                profile: payload.profile,
                page: payload.page,
            },
            Command::SendToPropertyInspector { context, payload } => {
                if !owned(self, &context) {
                    return;
                }
                if let Some(inspector) = self.inspectors.get(&context)
                    && let Some(conn) = inspector.conn
                {
                    out.push(Output::Send(
                        conn,
                        to_json(&InspectorEvent::SendToPropertyInspector {
                            action: inspector.action.clone(),
                            context,
                            payload,
                        }),
                    ));
                }
                return;
            }
            Command::OpenUrl { payload } => Request::OpenUrl {
                plugin,
                url: payload.url,
            },
            Command::LogMessage { payload } => {
                self.write_log(uuid, Source::Plugin, &payload.message);
                return;
            }
            Command::SendToPlugin { .. } => {
                self.write_log(
                    uuid,
                    Source::Host,
                    "ignored sendToPlugin, which is for property inspectors",
                );
                return;
            }
            Command::SetFeedback | Command::SetFeedbackLayout | Command::SetTriggerDescription => {
                return;
            }
        };
        out.push(Output::Request(request));
    }

    #[expect(clippy::too_many_lines, reason = "one arm per command")]
    fn inspector_command(&mut self, out: &mut Vec<Output>, conn: ConnId, own: &str, text: &str) {
        let Some(inspector) = self.inspectors.get(own) else {
            return;
        };
        let plugin = inspector.plugin.clone();
        let action = inspector.action.clone();
        let command = match Command::parse(text) {
            Ok(command) => command,
            Err(e) => {
                self.write_log(
                    &plugin,
                    Source::Host,
                    &format!("ignored a message from the property inspector: {e}"),
                );
                return;
            }
        };
        let owner = plugin.clone();
        let mine = |router: &Self, context: &str| {
            let ok = context == own;
            if !ok {
                router.write_log(
                    &owner,
                    Source::Host,
                    &format!("refused a property inspector message for context {context:?}, which isn't its own"),
                );
            }
            ok
        };
        let request = match command {
            Command::SetSettings { context, payload } if mine(self, &context) => {
                Request::SetSettings {
                    plugin,
                    context,
                    settings: payload,
                    from: Sender::PropertyInspector,
                }
            }
            Command::GetSettings { context, id } if mine(self, &context) => Request::GetSettings {
                plugin,
                context,
                id,
                from: Sender::PropertyInspector,
            },
            Command::SetResources { context, payload } if mine(self, &context) => {
                Request::SetResources {
                    plugin,
                    context,
                    resources: payload,
                    from: Sender::PropertyInspector,
                }
            }
            Command::GetResources { context, id } if mine(self, &context) => {
                Request::GetResources {
                    plugin,
                    context,
                    id,
                    from: Sender::PropertyInspector,
                }
            }
            Command::SetGlobalSettings { payload } => {
                self.set_global(&plugin, payload.clone());
                if let Some(conn) = self.plugins.get(&plugin).and_then(|slot| slot.conn) {
                    out.push(Output::Send(
                        conn,
                        to_json(&Event::DidReceiveGlobalSettings {
                            payload: GlobalSettingsPayload { settings: payload },
                            id: None,
                        }),
                    ));
                }
                return;
            }
            Command::GetGlobalSettings { id } => {
                let settings = self.get_global(&plugin);
                out.push(Output::Send(
                    conn,
                    to_json(&InspectorEvent::DidReceiveGlobalSettings {
                        payload: GlobalSettingsPayload { settings },
                        id,
                    }),
                ));
                return;
            }
            Command::SendToPlugin { context, payload } if mine(self, &context) => {
                if let Some(conn) = self.plugins.get(&plugin).and_then(|slot| slot.conn) {
                    out.push(Output::Send(
                        conn,
                        to_json(&Event::SendToPlugin {
                            action,
                            context,
                            payload,
                        }),
                    ));
                }
                return;
            }
            Command::OpenUrl { payload } => Request::OpenUrl {
                plugin,
                url: payload.url,
            },
            Command::LogMessage { payload } => {
                self.write_log(&plugin, Source::Inspector, &payload.message);
                return;
            }
            Command::SetSettings { .. }
            | Command::GetSettings { .. }
            | Command::SetResources { .. }
            | Command::GetResources { .. }
            | Command::SendToPlugin { .. } => return,
            other => {
                self.write_log(
                    &plugin,
                    Source::Host,
                    &format!(
                        "ignored {}, which property inspectors can't send",
                        command_name(&other)
                    ),
                );
                return;
            }
        };
        out.push(Output::Request(request));
    }

    /// Closes a plugin's connection, keeping its token.
    fn stopped_connection(&mut self, uuid: &str) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(slot) = self.plugins.get_mut(uuid)
            && let Some(conn) = slot.conn.take()
        {
            self.peers.remove(&conn);
            out.push(Output::Close(conn));
            out.push(Output::Request(Request::Disconnected {
                plugin: uuid.to_owned(),
            }));
        }
        out
    }

    /// Sends an event to a plugin, if it's connected. The plugin may
    /// address the event's context from now on.
    pub fn send(&mut self, plugin: &str, event: &Event) -> Vec<Output> {
        let Some(slot) = self.plugins.get_mut(plugin) else {
            return Vec::new();
        };
        if let Some(context) = event.context()
            && !slot.contexts.contains(context)
        {
            slot.contexts.insert(context.to_owned());
        }
        match slot.conn {
            Some(conn) => vec![Output::Send(conn, to_json(event))],
            None => Vec::new(),
        }
    }

    /// Sends an event to the property inspector of a context, if it's
    /// connected.
    pub fn send_to_inspector(&self, context: &str, event: &InspectorEvent) -> Vec<Output> {
        match self
            .inspectors
            .get(context)
            .and_then(|inspector| inspector.conn)
        {
            Some(conn) => vec![Output::Send(conn, to_json(event))],
            None => Vec::new(),
        }
    }

    /// Sends an event to every connected plugin.
    pub fn broadcast(&self, event: &Event) -> Vec<Output> {
        let text = to_json(event);
        self.plugins
            .values()
            .filter_map(|slot| slot.conn)
            .map(|conn| Output::Send(conn, text.clone()))
            .collect()
    }

    /// A device was connected, or changed.
    pub fn device_connected(&mut self, device: Device) -> Vec<Output> {
        let event = Event::DeviceDidConnect {
            device: device.id.clone(),
            device_info: device.info.clone(),
        };
        match self.devices.iter_mut().find(|known| known.id == device.id) {
            Some(known) => *known = device,
            None => self.devices.push(device),
        }
        self.broadcast(&event)
    }

    /// A device was disconnected.
    pub fn device_disconnected(&mut self, id: &str) -> Vec<Output> {
        let before = self.devices.len();
        self.devices.retain(|device| device.id != id);
        if self.devices.len() == before {
            return Vec::new();
        }
        self.broadcast(&Event::DeviceDidDisconnect {
            device: id.to_owned(),
        })
    }

    /// Opens the property inspector of an instance, replacing any open for
    /// it. The plugin may address the context from now on.
    pub fn open_inspector(
        &mut self,
        plugin: &str,
        action: &str,
        context: &str,
        device: &str,
        payload: &ActionPayload,
    ) -> Option<(InspectorLaunch, Vec<Output>)> {
        #[derive(Serialize)]
        struct ActionInfo<'a> {
            action: &'a str,
            context: &'a str,
            device: &'a str,
            payload: &'a ActionPayload,
        }
        let out = self.close_inspector(context);
        let slot = self.plugins.get_mut(plugin)?;
        slot.contexts.insert(context.to_owned());
        let info = registration_info(
            &self.app,
            &slot.plugin.uuid,
            &slot.plugin.manifest.version,
            &self.devices,
        );
        let action_info = to_json(&ActionInfo {
            action,
            context,
            device,
            payload,
        });
        self.inspectors.insert(
            context.to_owned(),
            Inspector {
                plugin: plugin.to_owned(),
                action: action.to_owned(),
                device: device.to_owned(),
                conn: None,
            },
        );
        Some((
            InspectorLaunch {
                port: self.port,
                uuid: context.to_owned(),
                register_event: REGISTER_INSPECTOR.to_owned(),
                info: info.to_string(),
                action_info,
            },
            out,
        ))
    }

    /// Closes the property inspector of an instance.
    pub fn close_inspector(&mut self, context: &str) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(inspector) = self.inspectors.get(context)
            && let Some(conn) = inspector.conn
        {
            self.peers.remove(&conn);
            out.push(Output::Close(conn));
            self.inspector_disappeared(&mut out, context);
        }
        self.inspectors.remove(context);
        out
    }

    fn inspector_appeared(&mut self, out: &mut Vec<Output>, context: &str) {
        let Some(inspector) = self.inspectors.get(context) else {
            return;
        };
        let (plugin, event) = (
            inspector.plugin.clone(),
            Event::PropertyInspectorDidAppear(Instance {
                action: inspector.action.clone(),
                context: context.to_owned(),
                device: inspector.device.clone(),
            }),
        );
        out.extend(self.send(&plugin, &event));
    }

    fn inspector_disappeared(&mut self, out: &mut Vec<Output>, context: &str) {
        let Some(inspector) = self.inspectors.get(context) else {
            return;
        };
        let (plugin, event) = (
            inspector.plugin.clone(),
            Event::PropertyInspectorDidDisappear(Instance {
                action: inspector.action.clone(),
                context: context.to_owned(),
                device: inspector.device.clone(),
            }),
        );
        out.extend(self.send(&plugin, &event));
    }

    fn get_global(&mut self, plugin: &str) -> Settings {
        let (settings, problem) = self.global.get(plugin);
        if let Some(problem) = problem {
            self.write_log(plugin, Source::Host, &problem);
        }
        settings
    }

    fn set_global(&mut self, plugin: &str, settings: Settings) {
        if let Err(problem) = self.global.set(plugin, settings) {
            self.write_log(plugin, Source::Host, &problem);
        }
    }

    fn write_log(&self, plugin: &str, source: Source, text: &str) {
        if let Some(slot) = self.plugins.get(plugin) {
            slot.log.write(source, text);
        }
    }
}

fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("messages serialize")
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::SetTitle { .. } => "setTitle",
        Command::SetImage { .. } => "setImage",
        Command::SetState { .. } => "setState",
        Command::ShowAlert { .. } => "showAlert",
        Command::ShowOk { .. } => "showOk",
        Command::SwitchToProfile { .. } => "switchToProfile",
        Command::SendToPropertyInspector { .. } => "sendToPropertyInspector",
        Command::SetFeedback => "setFeedback",
        Command::SetFeedbackLayout => "setFeedbackLayout",
        Command::SetTriggerDescription => "setTriggerDescription",
        _ => "a command",
    }
}

/// Settings as JSON, for tests.
#[cfg(test)]
pub(crate) fn settings(value: Value) -> Settings {
    match value {
        Value::Object(map) => map,
        _ => panic!("not an object"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::message::{ActionEvent, Coordinates, DeviceInfo, Size};

    const PLUGIN: &str = "com.example.a";
    const OTHER: &str = "com.example.b";

    struct Fixture {
        router: Router,
        _temp: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let mut plugins = Vec::new();
        for uuid in [PLUGIN, OTHER] {
            let dir = temp.path().join(format!("plugins/{uuid}.sdPlugin"));
            fs::create_dir_all(dir.join("imgs")).unwrap();
            fs::write(
                dir.join("manifest.json"),
                r#"{ "Name": "A", "Version": "1.0.0.0", "Actions": [] }"#,
            )
            .unwrap();
            fs::write(dir.join("imgs/on.svg"), "<svg/>").unwrap();
            plugins.push((Arc::new(Plugin::load(&dir).unwrap()), PluginLog::discard()));
        }
        let app = AppInfo {
            language: "en".to_owned(),
            font: "Noto Sans".to_owned(),
            platform_version: "6.0".to_owned(),
        };
        let global = GlobalSettings::new(temp.path().join("config"));
        Fixture {
            router: Router::new(4242, app, plugins, global),
            _temp: temp,
        }
    }

    fn device() -> Device {
        Device {
            id: "D1".to_owned(),
            info: DeviceInfo {
                name: "Stream Deck MK.2".to_owned(),
                size: Size {
                    columns: 5,
                    rows: 3,
                },
                kind: 0,
            },
        }
    }

    fn will_appear(context: &str) -> Event {
        Event::WillAppear(ActionEvent {
            action: format!("{PLUGIN}.action"),
            context: context.to_owned(),
            device: "D1".to_owned(),
            payload: payload(),
        })
    }

    fn payload() -> ActionPayload {
        ActionPayload {
            settings: settings(json!({ "n": 1 })),
            coordinates: Some(Coordinates { column: 1, row: 2 }),
            state: None,
            user_desired_state: None,
            resources: Resources::new(),
        }
    }

    fn sent(out: &[Output]) -> Vec<(ConnId, Value)> {
        out.iter()
            .filter_map(|output| match output {
                Output::Send(conn, text) => Some((*conn, serde_json::from_str(text).unwrap())),
                _ => None,
            })
            .collect()
    }

    fn requests(out: Vec<Output>) -> Vec<Request> {
        out.into_iter()
            .filter_map(|output| match output {
                Output::Request(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    /// Launches and registers a plugin on `conn`.
    fn connect(router: &mut Router, plugin: &str, conn: ConnId) -> Vec<Output> {
        let (launch, _) = router.launch(plugin).unwrap();
        router.opened(conn);
        let out = router.received(
            conn,
            &json!({ "event": "registerPlugin", "uuid": launch.token }).to_string(),
        );
        assert!(router.is_connected(plugin));
        out
    }

    #[test]
    fn launch_arguments() {
        let mut f = fixture();
        f.router.device_connected(device());
        let (launch, out) = f.router.launch(PLUGIN).unwrap();
        assert!(out.is_empty());
        let args = launch.args();
        assert_eq!(args[..3], ["-port", "4242", "-pluginUUID"]);
        assert_eq!(args[4..7], ["-registerEvent", "registerPlugin", "-info"]);
        let info: Value = serde_json::from_str(&args[7]).unwrap();
        assert_eq!(
            info["plugin"],
            json!({ "uuid": PLUGIN, "version": "1.0.0.0" })
        );
        assert_eq!(info["application"]["platform"], "linux");
        assert_eq!(info["devices"][0]["id"], "D1");
        assert_eq!(
            info["devices"][0]["size"],
            json!({ "columns": 5, "rows": 3 })
        );
        // A new token each start.
        let (again, _) = f.router.launch(PLUGIN).unwrap();
        assert_ne!(again.token, launch.token);
        assert!(f.router.launch("com.example.unknown").is_none());
    }

    #[test]
    fn registration() {
        let mut f = fixture();
        f.router.device_connected(device());
        let out = connect(&mut f.router, PLUGIN, 1);
        assert_eq!(
            sent(&out),
            [(
                1,
                json!({
                    "event": "deviceDidConnect",
                    "device": "D1",
                    "deviceInfo": { "name": "Stream Deck MK.2", "size": { "columns": 5, "rows": 3 }, "type": 0 }
                })
            )]
        );
        assert_eq!(
            requests(out),
            [Request::Registered {
                plugin: PLUGIN.to_owned()
            }]
        );

        // Unknown tokens, the same token twice, and garbage are refused.
        for (conn, text) in [
            (
                2,
                json!({ "event": "registerPlugin", "uuid": "guess" }).to_string(),
            ),
            (
                3,
                json!({ "event": "registerPropertyInspector", "uuid": "guess" }).to_string(),
            ),
            (4, "hello".to_owned()),
        ] {
            f.router.opened(conn);
            assert_eq!(f.router.received(conn, &text), [Output::Close(conn)]);
            assert!(f.router.received(conn, &text).is_empty());
        }

        // An old token stops working once the plugin starts again.
        let (launch, _) = f.router.launch(OTHER).unwrap();
        f.router.launch(OTHER).unwrap();
        f.router.opened(5);
        let old = json!({ "event": "registerPlugin", "uuid": launch.token }).to_string();
        assert_eq!(f.router.received(5, &old), [Output::Close(5)]);
    }

    #[test]
    fn restarting_closes_the_old_connection() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        let (_, out) = f.router.launch(PLUGIN).unwrap();
        assert_eq!(
            out,
            [
                Output::Close(1),
                Output::Request(Request::Disconnected {
                    plugin: PLUGIN.to_owned()
                })
            ]
        );
        assert!(!f.router.is_connected(PLUGIN));
        // Its close is already handled.
        assert!(f.router.closed(1).is_empty());
    }

    #[test]
    fn registering_twice_disconnects() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        let out = f
            .router
            .received(1, r#"{"event":"registerPlugin","uuid":"x"}"#);
        assert_eq!(out[0], Output::Close(1));
        assert!(!f.router.is_connected(PLUGIN));
    }

    #[test]
    fn events_reach_connected_plugins_only() {
        let mut f = fixture();
        // Not connected yet: dropped.
        assert!(f.router.send(PLUGIN, &will_appear("c1")).is_empty());
        connect(&mut f.router, PLUGIN, 1);
        let out = f.router.send(PLUGIN, &will_appear("c1"));
        assert_eq!(sent(&out)[0].1["event"], "willAppear");
        assert_eq!(
            sent(&out)[0].1["payload"],
            json!({
                "settings": { "n": 1 },
                "coordinates": { "column": 1, "row": 2 },
                "controller": "Keypad",
                "isInMultiAction": false,
                "resources": {}
            })
        );
        assert_eq!(
            requests(f.router.closed(1)),
            [Request::Disconnected {
                plugin: PLUGIN.to_owned()
            }]
        );
        assert!(f.router.send(PLUGIN, &will_appear("c1")).is_empty());
    }

    #[test]
    fn plugins_address_only_their_contexts() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        connect(&mut f.router, OTHER, 2);
        f.router.send(PLUGIN, &will_appear("mine"));

        let set_title = |context: &str| {
            json!({ "event": "setTitle", "context": context, "payload": { "title": "Hi", "state": 1 } })
                .to_string()
        };
        assert_eq!(
            requests(f.router.received(1, &set_title("mine"))),
            [Request::SetTitle {
                plugin: PLUGIN.to_owned(),
                context: "mine".to_owned(),
                state: Some(1),
                title: Some("Hi".to_owned()),
            }]
        );
        // Another plugin's context, or one never seen.
        assert!(f.router.received(2, &set_title("mine")).is_empty());
        assert!(f.router.received(1, &set_title("unknown")).is_empty());
    }

    #[test]
    fn commands_become_requests() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        f.router.send(PLUGIN, &will_appear("c"));
        let cases = [
            (
                json!({ "event": "setSettings", "context": "c", "payload": { "a": 1 } }),
                Request::SetSettings {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                    settings: settings(json!({ "a": 1 })),
                    from: Sender::Plugin,
                },
            ),
            (
                json!({ "event": "getSettings", "context": "c", "id": "7" }),
                Request::GetSettings {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                    id: Some("7".to_owned()),
                    from: Sender::Plugin,
                },
            ),
            (
                json!({ "event": "setImage", "context": "c", "payload": { "image": "imgs/on", "target": 0 } }),
                Request::SetImage {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                    state: None,
                    image: Some("data:image/svg+xml;base64,PHN2Zy8+".to_owned()),
                },
            ),
            (
                json!({ "event": "setImage", "context": "c", "payload": { "image": "" } }),
                Request::SetImage {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                    state: None,
                    image: None,
                },
            ),
            (
                json!({ "event": "setState", "context": "c", "payload": { "state": 1 } }),
                Request::SetState {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                    state: 1,
                },
            ),
            (
                json!({ "event": "showAlert", "context": "c" }),
                Request::ShowAlert {
                    plugin: PLUGIN.to_owned(),
                    context: "c".to_owned(),
                },
            ),
            (
                json!({ "event": "switchToProfile", "context": "token", "device": "D1", "payload": { "profile": "Main" } }),
                Request::SwitchToProfile {
                    plugin: PLUGIN.to_owned(),
                    device: "D1".to_owned(),
                    profile: Some("Main".to_owned()),
                    page: None,
                },
            ),
            (
                json!({ "event": "openUrl", "payload": { "url": "https://example.com" } }),
                Request::OpenUrl {
                    plugin: PLUGIN.to_owned(),
                    url: "https://example.com".to_owned(),
                },
            ),
        ];
        for (command, request) in cases {
            assert_eq!(
                requests(f.router.received(1, &command.to_string())),
                [request],
                "{command}"
            );
        }
        // A path outside the plugin is refused.
        let escape =
            json!({ "event": "setImage", "context": "c", "payload": { "image": "../../x.svg" } });
        assert!(f.router.received(1, &escape.to_string()).is_empty());
        // Ignored.
        for ignored in [
            json!({ "event": "setFeedback", "context": "c", "payload": {} }),
            json!({ "event": "logMessage", "payload": { "message": "hi" } }),
            json!({ "event": "teleport" }),
        ] {
            assert!(
                f.router.received(1, &ignored.to_string()).is_empty(),
                "{ignored}"
            );
        }
    }

    #[test]
    fn global_settings() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        let get =
            json!({ "event": "getGlobalSettings", "context": "token", "id": "q" }).to_string();
        assert_eq!(
            sent(&f.router.received(1, &get)),
            [(
                1,
                json!({ "event": "didReceiveGlobalSettings", "payload": { "settings": {} }, "id": "q" })
            )]
        );
        let set =
            json!({ "event": "setGlobalSettings", "context": "token", "payload": { "key": "v" } });
        assert!(f.router.received(1, &set.to_string()).is_empty());
        assert_eq!(
            sent(&f.router.received(1, &get))[0].1["payload"]["settings"],
            json!({ "key": "v" })
        );
        // Each plugin has its own.
        connect(&mut f.router, OTHER, 2);
        assert_eq!(
            sent(&f.router.received(2, &get))[0].1["payload"]["settings"],
            json!({})
        );
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one inspector's whole life")]
    fn property_inspectors() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        let action = format!("{PLUGIN}.action");
        let (launch, out) = f
            .router
            .open_inspector(PLUGIN, &action, "c", "D1", &payload())
            .unwrap();
        assert!(out.is_empty());
        assert_eq!(launch.uuid, "c");
        assert_eq!(launch.register_event, "registerPropertyInspector");
        let action_info: Value = serde_json::from_str(&launch.action_info).unwrap();
        assert_eq!(action_info["context"], "c");
        assert_eq!(action_info["payload"]["settings"], json!({ "n": 1 }));

        // Registering tells the plugin.
        f.router.opened(9);
        let out = f
            .router
            .received(9, r#"{"event":"registerPropertyInspector","uuid":"c"}"#);
        assert_eq!(
            sent(&out),
            [(
                1,
                json!({ "event": "propertyInspectorDidAppear", "action": action, "context": "c", "device": "D1" })
            )]
        );
        // Only one connection per inspector.
        f.router.opened(10);
        assert_eq!(
            f.router
                .received(10, r#"{"event":"registerPropertyInspector","uuid":"c"}"#),
            [Output::Close(10)]
        );

        // Settings from the inspector go to the app, marked as such.
        assert_eq!(
            requests(f.router.received(
                9,
                r#"{"event":"setSettings","context":"c","payload":{"x":2}}"#
            )),
            [Request::SetSettings {
                plugin: PLUGIN.to_owned(),
                context: "c".to_owned(),
                settings: settings(json!({ "x": 2 })),
                from: Sender::PropertyInspector,
            }]
        );
        // Not for other contexts, and not plugin-only commands.
        assert!(
            f.router
                .received(9, r#"{"event":"setSettings","context":"d","payload":{}}"#)
                .is_empty()
        );
        assert!(
            f.router
                .received(
                    9,
                    r#"{"event":"setTitle","context":"c","payload":{"title":"x"}}"#
                )
                .is_empty()
        );

        // Messages both ways.
        assert_eq!(
            sent(&f.router.received(
                9,
                r#"{"event":"sendToPlugin","action":"x","context":"c","payload":[1]}"#
            )),
            [(
                1,
                json!({ "event": "sendToPlugin", "action": action, "context": "c", "payload": [1] })
            )]
        );
        assert_eq!(
            sent(&f.router.received(
                1,
                r#"{"event":"sendToPropertyInspector","context":"c","payload":{"y":true}}"#
            )),
            [(
                9,
                json!({ "event": "sendToPropertyInspector", "action": action, "context": "c", "payload": { "y": true } })
            )]
        );

        // Global settings saved on one side reach the other.
        assert_eq!(
            sent(&f.router.received(
                9,
                r#"{"event":"setGlobalSettings","context":"c","payload":{"g":1}}"#
            )),
            [(
                1,
                json!({ "event": "didReceiveGlobalSettings", "payload": { "settings": { "g": 1 } } })
            )]
        );
        assert_eq!(
            sent(&f.router.received(
                1,
                r#"{"event":"setGlobalSettings","context":"t","payload":{"g":2}}"#
            )),
            [(
                9,
                json!({ "event": "didReceiveGlobalSettings", "payload": { "settings": { "g": 2 } } })
            )]
        );

        // Closing it.
        let out = f.router.close_inspector("c");
        assert_eq!(out[0], Output::Close(9));
        assert_eq!(sent(&out)[0].1["event"], "propertyInspectorDidDisappear");
        assert!(f.router.closed(9).is_empty());
        f.router.opened(11);
        assert_eq!(
            f.router
                .received(11, r#"{"event":"registerPropertyInspector","uuid":"c"}"#),
            [Output::Close(11)]
        );
    }

    #[test]
    fn devices_come_and_go() {
        let mut f = fixture();
        connect(&mut f.router, PLUGIN, 1);
        let out = f.router.device_connected(device());
        assert_eq!(sent(&out)[0].1["event"], "deviceDidConnect");
        assert_eq!(
            sent(&f.router.device_disconnected("D1")),
            [(1, json!({ "event": "deviceDidDisconnect", "device": "D1" }))]
        );
        assert!(f.router.device_disconnected("D1").is_empty());
    }
}
