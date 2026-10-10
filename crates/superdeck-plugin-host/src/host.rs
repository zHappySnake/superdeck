// SPDX-License-Identifier: GPL-3.0-or-later

//! [`PluginHost`], the app's handle on the running host.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;

use crate::Error;
use crate::global_settings::GlobalSettings;
use crate::info::AppInfo;
use crate::log::{PluginLog, Source};
use crate::manifest::{Entry, Plugin};
use crate::message::{ActionPayload, Device, Event, InspectorEvent};
use crate::node::NodeRuntime;
use crate::process::{self, Options};
use crate::router::{InspectorLaunch, PluginLaunch, Request, Router};
use crate::server::{self, Input};

/// How plugins are started.
#[derive(Debug, Clone)]
pub enum Launch {
    /// SuperDeck starts every Node.js plugin, and starts it again when it
    /// exits.
    Node(NodeRuntime),
    /// Nothing is started; [`PluginHost::launch_manually`] gives the
    /// arguments to start a plugin by hand, under a debugger say.
    Manual,
}

/// What the host needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    /// The plugins, from [`discover`](crate::discover).
    pub plugins: Vec<Plugin>,
    /// Where global settings are kept: `<dir>/<uuid>/settings.json`.
    /// Usually `$XDG_CONFIG_HOME/superdeck/plugins`.
    pub settings_dir: PathBuf,
    /// Where plugin logs go. Usually `$XDG_STATE_HOME/superdeck/logs/plugins`.
    pub log_dir: PathBuf,
    /// How plugins are started.
    pub launch: Launch,
    /// About the app, for plugins.
    pub app: AppInfo,
    /// `Origin`s property inspectors connect from, such as the app's
    /// custom URI scheme. Connections with any other `Origin` are refused.
    pub inspector_origins: Vec<String>,
    /// Honor the manifests' `Nodejs.Debug`.
    pub debug: bool,
}

/// The running plugin host: a WebSocket server on `127.0.0.1` and the
/// plugin processes. Calls never block; requests from plugins arrive
/// through [`recv`](Self::recv) and [`try_recv`](Self::try_recv).
///
/// It must be started, and dropped or [shut down](Self::shutdown), within
/// a Tokio runtime.
#[derive(Debug)]
pub struct PluginHost {
    port: u16,
    inputs: mpsc::UnboundedSender<Input>,
    requests: mpsc::UnboundedReceiver<Request>,
    shutdown: watch::Sender<bool>,
    /// The router and the server.
    server: JoinSet<()>,
    /// One per plugin process.
    supervisors: JoinSet<()>,
    logs: Vec<(String, PathBuf)>,
    warnings: Vec<String>,
}

impl PluginHost {
    /// Starts the server, then every plugin that can run.
    pub async fn start(config: Config) -> Result<Self, Error> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(Error::Server)?;
        let port = listener.local_addr().map_err(Error::Server)?.port();

        let mut plugins = Vec::new();
        let mut logs = Vec::new();
        let mut warnings = Vec::new();
        for plugin in config.plugins {
            let (log, error) = PluginLog::open(&config.log_dir, &plugin.uuid);
            if let Some(error) = error {
                // Logging is best effort; the plugin still runs.
                warnings.push(format!("{}: no log, {error}", log.path().display()));
            }
            if let Entry::Unsupported(why) = &plugin.entry {
                log.write(Source::Host, &format!("not started: {why}"));
            }
            logs.push((plugin.uuid.clone(), log.path()));
            plugins.push((Arc::new(plugin), log));
        }

        let (inputs, input_queue) = mpsc::unbounded_channel();
        let (request_sender, requests) = mpsc::unbounded_channel();
        let (shutdown, _) = watch::channel(false);
        let router = Router::new(
            port,
            config.app,
            plugins
                .iter()
                .map(|(plugin, log)| (Arc::clone(plugin), log.clone())),
            GlobalSettings::new(config.settings_dir),
        );
        let mut server = JoinSet::new();
        server.spawn(server::route(router, input_queue, request_sender));
        server.spawn(server::accept(
            listener,
            inputs.clone(),
            config.inspector_origins.into(),
        ));
        let mut supervisors = JoinSet::new();
        if let Launch::Node(node) = config.launch {
            let options = Options {
                node,
                debug: config.debug,
            };
            for (plugin, log) in plugins {
                if matches!(plugin.entry, Entry::Node { .. }) {
                    supervisors.spawn(process::supervise(
                        plugin,
                        log,
                        options.clone(),
                        inputs.clone(),
                        shutdown.subscribe(),
                    ));
                }
            }
        }
        Ok(Self {
            port,
            inputs,
            requests,
            shutdown,
            server,
            supervisors,
            logs,
            warnings,
        })
    }

    /// The port of the WebSocket server.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Each plugin's log file.
    #[must_use]
    pub fn logs(&self) -> &[(String, PathBuf)] {
        &self.logs
    }

    /// Problems starting that didn't stop the host, such as a log file
    /// that couldn't be opened.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn input(&self, input: Input) {
        // Only fails once the host is shutting down.
        let _ = self.inputs.send(input);
    }

    /// Sends an event to a plugin, if it's connected; events for plugins
    /// that aren't are dropped. The plugin may address the event's context
    /// from now on.
    pub fn send(&self, plugin: &str, event: Event) {
        self.input(Input::Send {
            plugin: plugin.to_owned(),
            event,
        });
    }

    /// Sends an event to the property inspector of an instance, if it's
    /// connected.
    pub fn send_to_inspector(&self, context: &str, event: InspectorEvent) {
        self.input(Input::SendToInspector {
            context: context.to_owned(),
            event,
        });
    }

    /// Tells every plugin that the computer resumed from suspend.
    pub fn system_did_wake_up(&self) {
        self.input(Input::Broadcast(Event::SystemDidWakeUp));
    }

    /// A device was connected, or changed. Plugins started from now on
    /// see it in their registration info, and connected ones get
    /// `deviceDidConnect`.
    pub fn device_connected(&self, device: Device) {
        self.input(Input::DeviceConnected(device));
    }

    /// A device was disconnected.
    pub fn device_disconnected(&self, id: &str) {
        self.input(Input::DeviceDisconnected(id.to_owned()));
    }

    /// Opens the property inspector of an instance, replacing any open for
    /// it, and returns what the inspector's page needs to connect. `None`
    /// for an unknown plugin.
    pub async fn open_property_inspector(
        &self,
        plugin: &str,
        action: &str,
        context: &str,
        device: &str,
        payload: ActionPayload,
    ) -> Option<InspectorLaunch> {
        let (reply, launch) = oneshot::channel();
        self.input(Input::OpenInspector {
            plugin: plugin.to_owned(),
            action: action.to_owned(),
            context: context.to_owned(),
            device: device.to_owned(),
            payload,
            reply,
        });
        launch.await.ok().flatten()
    }

    /// Closes the property inspector of an instance.
    pub fn close_property_inspector(&self, context: &str) {
        self.input(Input::CloseInspector {
            context: context.to_owned(),
        });
    }

    /// With [`Launch::Manual`], the arguments to start a plugin by hand. A
    /// new token each time: only the latest registers. `None` for an
    /// unknown plugin.
    pub async fn launch_manually(&self, plugin: &str) -> Option<PluginLaunch> {
        let (reply, launch) = oneshot::channel();
        self.input(Input::Launch {
            plugin: plugin.to_owned(),
            reply,
        });
        launch.await.ok().flatten()
    }

    /// The next request from a plugin, waiting for one.
    pub async fn recv(&mut self) -> Option<Request> {
        self.requests.recv().await
    }

    /// The next request from a plugin, if there's one.
    pub fn try_recv(&mut self) -> Option<Request> {
        self.requests.try_recv().ok()
    }

    /// Stops every plugin, waiting for them to exit, and the server.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        while self.supervisors.join_next().await.is_some() {}
        self.server.shutdown().await;
    }
}
