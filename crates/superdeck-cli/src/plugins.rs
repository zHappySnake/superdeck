// SPDX-License-Identifier: GPL-3.0-or-later

//! `superdeck-cli run` and plugins: starting the plugin host, and
//! translating between a [`Deck`] and the plugin protocol.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use superdeck_core::deck::{
    ActionDefaults, Catalog, FaceImage, PluginEvent, PluginEventKind, StateDefaults,
};
use superdeck_core::model::{Color, FontStyle, TitleAlignment};
use superdeck_core::{ConfigDir, Deck, Effect, InstanceId, Position};
use superdeck_device::Model;
use superdeck_plugin_host::message::{
    self, ActionEvent, ActionPayload, Coordinates, Device, DeviceInfo, Event, InspectorEvent, Size,
    TitleEvent, TitleParameters, TitlePayload,
};
use superdeck_plugin_host::{
    AppInfo, Config, Entry, Launch, NodeRuntime, Plugin, PluginHost, Request, Sender, discover,
    plugin_dirs,
};

/// Plugin options of the `run` command.
#[derive(clap::Args)]
pub struct PluginArgs {
    /// Don't run plugins; print their events instead.
    #[arg(long)]
    no_plugins: bool,
    /// Start no plugins, and print the command to start each one by hand,
    /// under a debugger say.
    #[arg(long, conflicts_with = "no_plugins")]
    manual_plugins: bool,
    /// Honor the manifests' `Nodejs.Debug`, starting plugins with the
    /// Node.js inspector.
    #[arg(long, conflicts_with = "no_plugins")]
    debug_plugins: bool,
    /// Another directory of `<uuid>.sdPlugin` plugins, which win over the
    /// installed ones. Repeat for more.
    #[arg(long, value_name = "DIR")]
    plugin_dir: Vec<PathBuf>,
}

/// The device as plugins see it.
pub fn device(model: Model, serial: &str, columns: u8, rows: u8) -> Device {
    Device {
        id: device_id(serial),
        info: DeviceInfo {
            name: model.to_string(),
            size: Size { columns, rows },
            kind: device_type(model),
        },
    }
}

/// The first 16 bytes of the SHA-256 hash of the serial number, in
/// uppercase hexadecimal.
fn device_id(serial: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(serial.as_bytes())[..16].iter().fold(
        String::with_capacity(32),
        |mut hex, byte| {
            let _ = write!(hex, "{byte:02X}");
            hex
        },
    )
}

/// The SDK's device type.
fn device_type(model: Model) -> u8 {
    match model {
        Model::Mini | Model::MiniMk2 | Model::Module6 => 1,
        Model::Xl | Model::XlV2 | Model::Module32 => 2,
        Model::Neo => 9,
        // The Original, MK.2, and Module 15 are all "Stream Deck".
        _ => 0,
    }
}

/// The running plugins, and what the deck needs to know about them.
pub struct Plugins {
    /// Keeps the host's tasks running.
    runtime: tokio::runtime::Runtime,
    host: PluginHost,
    device: String,
}

/// Finds the plugins and starts the host, unless `--no-plugins`. Returns
/// the catalog of their actions either way.
pub fn start(
    args: &PluginArgs,
    config: &ConfigDir,
    device: Device,
) -> Result<(Option<Plugins>, Arc<Catalog>)> {
    let mut dirs = plugin_dirs();
    dirs.extend(args.plugin_dir.iter().cloned());
    let found = discover(&dirs);
    for problem in &found.problems {
        eprintln!("Plugin not loaded: {problem}");
    }
    let plugins: Vec<Plugin> = found.plugins.into_values().collect();
    let catalog = Arc::new(catalog(&plugins));
    if args.no_plugins {
        return Ok((None, catalog));
    }

    let launch = if args.manual_plugins {
        Launch::Manual
    } else {
        match NodeRuntime::find() {
            Ok(node) => Launch::Node(node),
            Err(error) => {
                eprintln!("Not starting plugins: {error}");
                Launch::Manual
            }
        }
    };
    for plugin in &plugins {
        match &plugin.entry {
            Entry::Node { .. } => eprintln!("Plugin {} ({})", plugin.uuid, plugin.manifest.name),
            Entry::Unsupported(why) => eprintln!("Plugin {} can't run: {why}", plugin.uuid),
        }
    }
    let runnable: Vec<(String, PathBuf)> = plugins
        .iter()
        .filter_map(|plugin| match &plugin.entry {
            Entry::Node { script, .. } => Some((plugin.uuid.clone(), script.clone())),
            Entry::Unsupported(_) => None,
        })
        .collect();

    let runtime = tokio::runtime::Runtime::new().context("could not start the async runtime")?;
    let host = runtime.block_on(PluginHost::start(Config {
        plugins,
        settings_dir: config.root().join("plugins"),
        log_dir: state_dir()
            .context("no home directory for plugin logs")?
            .join("logs/plugins"),
        launch: launch.clone(),
        app: AppInfo::from_system(),
        inspector_origins: Vec::new(),
        debug: args.debug_plugins,
    }))?;
    for warning in host.warnings() {
        eprintln!("{warning}");
    }
    if let Some((_, log)) = host.logs().first() {
        eprintln!(
            "Plugin logs are in {}",
            log.parent().unwrap_or(log).display()
        );
    }
    let id = device.id.clone();
    host.device_connected(device);
    if matches!(launch, Launch::Manual) {
        for (uuid, script) in runnable {
            if let Some(launch) = runtime.block_on(host.launch_manually(&uuid)) {
                let args: Vec<String> = launch.args().iter().map(|arg| shell_quote(arg)).collect();
                eprintln!(
                    "Start {uuid} with:\n  cd {} && node {} {}",
                    shell_quote(&script.parent().unwrap_or(&script).display().to_string()),
                    shell_quote(&script.display().to_string()),
                    args.join(" ")
                );
            }
        }
    }
    Ok((
        Some(Plugins {
            runtime,
            host,
            device: id,
        }),
        catalog,
    ))
}

fn state_dir() -> Option<PathBuf> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let state =
        absolute("XDG_STATE_HOME").or_else(|| Some(absolute("HOME")?.join(".local/state")))?;
    Some(state.join("superdeck"))
}

fn shell_quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@".contains(c))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// The deck's view of the plugins' actions, from their manifests.
fn catalog(plugins: &[Plugin]) -> Catalog {
    let mut catalog = HashMap::new();
    for plugin in plugins {
        for action in &plugin.manifest.actions {
            let states = action
                .states
                .iter()
                .map(|state| StateDefaults {
                    image: state
                        .image
                        .as_ref()
                        .and_then(|image| plugin.image_file(image))
                        .map(FaceImage::File),
                    title: state.title.clone(),
                    show_title: state.show_title,
                    title_alignment: match state.title_alignment.as_deref() {
                        Some("top") => Some(TitleAlignment::Top),
                        Some("middle") => Some(TitleAlignment::Middle),
                        Some("bottom") => Some(TitleAlignment::Bottom),
                        _ => None,
                    },
                    font_family: state
                        .font_family
                        .clone()
                        .filter(|family| !family.is_empty()),
                    font_size: state.font_size.map(|size| size.clamp(1.0, 144.0)),
                    font_style: match state.font_style.as_deref() {
                        Some("Regular") => Some(FontStyle::Regular),
                        Some("Bold") => Some(FontStyle::Bold),
                        Some("Italic") => Some(FontStyle::Italic),
                        Some("Bold Italic") => Some(FontStyle::BoldItalic),
                        _ => None,
                    },
                    font_underline: state.font_underline,
                    title_color: state
                        .title_color
                        .as_deref()
                        .and_then(|color| color.parse::<Color>().ok()),
                })
                .collect();
            catalog.insert(
                (plugin.uuid.clone(), action.uuid.clone()),
                ActionDefaults {
                    states,
                    automatic_states: !action.disable_automatic_states,
                },
            );
        }
    }
    catalog
}

impl Plugins {
    /// Sends a plugin event from the deck. A key's `willAppear` is followed
    /// by its title parameters.
    pub fn send(&self, deck: &Deck, event: &PluginEvent) {
        let about = self.action_event(event);
        let message = match event.kind {
            PluginEventKind::WillAppear => Event::WillAppear(about),
            PluginEventKind::WillDisappear => Event::WillDisappear(about),
            PluginEventKind::KeyDown => Event::KeyDown(about),
            PluginEventKind::KeyUp => Event::KeyUp(about),
            PluginEventKind::DidReceiveSettings => Event::DidReceiveSettings { about, id: None },
            PluginEventKind::DidReceiveResources => Event::DidReceiveResources { about, id: None },
        };
        self.host.send(&event.plugin, message);
        if event.kind == PluginEventKind::WillAppear
            && let Some(position) = event.position
            && let Some(face) = deck.face(position)
        {
            let parameters = TitleParameters {
                font_family: face.font_family.clone().unwrap_or_default(),
                font_size: face.font_size,
                font_style: match face.font_style {
                    FontStyle::Regular => message::FontStyle::Regular,
                    FontStyle::Bold => message::FontStyle::Bold,
                    FontStyle::Italic => message::FontStyle::Italic,
                    FontStyle::BoldItalic => message::FontStyle::BoldItalic,
                },
                font_underline: face.font_underline,
                show_title: face.show_title,
                title_alignment: match face.title_alignment {
                    TitleAlignment::Top => message::TitleAlignment::Top,
                    TitleAlignment::Middle => message::TitleAlignment::Middle,
                    TitleAlignment::Bottom => message::TitleAlignment::Bottom,
                },
                title_color: face.title_color.to_string(),
            };
            self.host.send(
                &event.plugin,
                Event::TitleParametersDidChange(TitleEvent {
                    action: event.action.clone(),
                    context: event.instance.to_string(),
                    device: self.device.clone(),
                    payload: TitlePayload {
                        settings: event.settings.clone(),
                        coordinates: coordinates(position),
                        state: (event.states > 1).then_some(event.state),
                        title: face.title,
                        title_parameters: parameters,
                        resources: event.resources.clone(),
                    },
                }),
            );
        }
    }

    fn action_event(&self, event: &PluginEvent) -> ActionEvent {
        let key_event = matches!(
            event.kind,
            PluginEventKind::KeyDown | PluginEventKind::KeyUp
        );
        ActionEvent {
            action: event.action.clone(),
            context: event.instance.to_string(),
            device: self.device.clone(),
            payload: ActionPayload {
                settings: event.settings.clone(),
                coordinates: event.position.map(coordinates),
                state: (event.states > 1).then_some(event.state),
                // A Multi Action step runs in the state the user chose.
                user_desired_state: (key_event && event.position.is_none()).then_some(event.state),
                resources: event.resources.clone(),
            },
        }
    }

    /// The next request from a plugin, if there's one.
    pub fn next_request(&mut self) -> Option<Request> {
        self.host.try_recv()
    }

    /// Carries out a request on the deck; returns the deck's effects.
    #[expect(clippy::too_many_lines, reason = "one arm per request")]
    pub fn handle(&self, deck: &mut Deck, request: Request, now: Instant) -> Vec<Effect> {
        match request {
            Request::Started { plugin } => eprintln!("Plugin {plugin} started"),
            Request::Stopped { plugin, reason } => eprintln!("Plugin {plugin} {reason}"),
            Request::Registered { plugin } => {
                eprintln!("Plugin {plugin} connected");
                return deck.announce(&plugin);
            }
            Request::Disconnected { plugin } => eprintln!("Plugin {plugin} disconnected"),
            Request::SetSettings {
                plugin,
                context,
                settings,
                from,
            } => {
                let Some(id) = owned(deck, &plugin, &context) else {
                    return Vec::new();
                };
                let effects = deck.set_settings(id, settings);
                self.answer(
                    deck,
                    PluginEventKind::DidReceiveSettings,
                    id,
                    None,
                    other(from),
                );
                return effects;
            }
            Request::GetSettings {
                plugin,
                context,
                id: request,
                from,
            } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    self.answer(deck, PluginEventKind::DidReceiveSettings, id, request, from);
                }
            }
            Request::SetResources {
                plugin,
                context,
                resources,
                from,
            } => {
                let Some(id) = owned(deck, &plugin, &context) else {
                    return Vec::new();
                };
                let effects = deck.set_resources(id, resources);
                self.answer(
                    deck,
                    PluginEventKind::DidReceiveResources,
                    id,
                    None,
                    other(from),
                );
                return effects;
            }
            Request::GetResources {
                plugin,
                context,
                id: request,
                from,
            } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    self.answer(
                        deck,
                        PluginEventKind::DidReceiveResources,
                        id,
                        request,
                        from,
                    );
                }
            }
            Request::SetTitle {
                plugin,
                context,
                state,
                title,
            } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    return deck.set_title(id, state, title.as_deref());
                }
            }
            Request::SetImage {
                plugin,
                context,
                state,
                image,
            } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    return deck.set_image(id, state, image.as_deref());
                }
            }
            Request::SetState {
                plugin,
                context,
                state,
            } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    return deck.set_state(id, state);
                }
            }
            Request::ShowAlert { plugin, context } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    return deck.show_alert(id, now);
                }
            }
            Request::ShowOk { plugin, context } => {
                if let Some(id) = owned(deck, &plugin, &context) {
                    return deck.show_ok(id, now);
                }
            }
            Request::SwitchToProfile { plugin, .. } => {
                eprintln!(
                    "Plugin {plugin} asked to switch profiles, which needs plugin profiles (M6)"
                );
            }
            Request::OpenUrl { plugin, url } => open_url(&plugin, &url),
            _ => {}
        }
        Vec::new()
    }

    /// Sends `didReceiveSettings` or `didReceiveResources` about an
    /// instance to the plugin or its property inspector.
    fn answer(
        &self,
        deck: &Deck,
        kind: PluginEventKind,
        id: InstanceId,
        request: Option<String>,
        to: Sender,
    ) {
        let Some(event) = deck.describe(kind, id) else {
            return;
        };
        let about = self.action_event(&event);
        let settings = kind == PluginEventKind::DidReceiveSettings;
        match to {
            Sender::Plugin => self.host.send(
                &event.plugin,
                if settings {
                    Event::DidReceiveSettings { about, id: request }
                } else {
                    Event::DidReceiveResources { about, id: request }
                },
            ),
            Sender::PropertyInspector => self.host.send_to_inspector(
                &id.to_string(),
                if settings {
                    InspectorEvent::DidReceiveSettings { about, id: request }
                } else {
                    InspectorEvent::DidReceiveResources { about, id: request }
                },
            ),
        }
    }

    /// Stops the plugins.
    pub fn shutdown(self) {
        let Plugins { runtime, host, .. } = self;
        runtime.block_on(host.shutdown());
    }
}

/// The instance a request names by context, if it's the plugin's.
fn owned(deck: &Deck, plugin: &str, context: &str) -> Option<InstanceId> {
    context
        .parse::<InstanceId>()
        .ok()
        .filter(|id| deck.owns(plugin, *id))
}

/// The side that didn't send a change, which hears about it.
fn other(from: Sender) -> Sender {
    match from {
        Sender::Plugin => Sender::PropertyInspector,
        Sender::PropertyInspector => Sender::Plugin,
    }
}

fn coordinates(position: Position) -> Coordinates {
    Coordinates {
        column: position.column,
        row: position.row,
    }
}

/// Opens web links in the default browser.
fn open_url(plugin: &str, url: &str) {
    let scheme = url
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https")) {
        eprintln!("Plugin {plugin} asked to open {url:?}; only web links are opened");
        return;
    }
    match std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("Could not open {url}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids() {
        let id = device_id("AL12H1A00001");
        assert_eq!(id.len(), 32);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        );
        assert_eq!(id, device_id("AL12H1A00001"));
        assert_ne!(id, device_id("AL12H1A00002"));
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("-port"), "-port");
        assert_eq!(shell_quote(r#"{"a": 1}"#), r#"'{"a": 1}'"#);
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
