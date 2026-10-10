// SPDX-License-Identifier: GPL-3.0-or-later

//! The host over real WebSocket connections, with plugins started by hand
//! and, when Node.js 24 is installed, by the host.

use std::fs;
use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use superdeck_plugin_host::message::{
    ActionEvent, ActionPayload, Coordinates, Device, DeviceInfo, Event, Resources, Settings, Size,
};
use superdeck_plugin_host::{
    AppInfo, Config, Launch, NodeRuntime, Plugin, PluginHost, Request, Sender,
};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const UUID: &str = "com.example.echo";
const WAIT: Duration = Duration::from_secs(10);

/// A plugin directory with `script` as its entry point.
fn write_plugin(root: &Path, script: &str) -> Plugin {
    let dir = root.join(format!("{UUID}.sdPlugin"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("manifest.json"),
        json!({
            "Name": "Echo",
            "Version": "1.0.0.0",
            "UUID": UUID,
            "CodePath": "plugin.mjs",
            "Nodejs": { "Version": "20" },
            "Software": { "MinimumVersion": "7.4" },
            "Actions": [{ "UUID": format!("{UUID}.say"), "Name": "Say", "States": [{}] }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(dir.join("plugin.mjs"), script).unwrap();
    Plugin::load(&dir).unwrap()
}

fn config(root: &Path, plugin: Plugin, launch: Launch) -> Config {
    Config {
        plugins: vec![plugin],
        settings_dir: root.join("config/plugins"),
        log_dir: root.join("logs"),
        launch,
        app: AppInfo {
            language: "en".to_owned(),
            font: "Noto Sans".to_owned(),
            platform_version: "6.0".to_owned(),
        },
        inspector_origins: vec!["superdeck://inspector".to_owned()],
        debug: false,
    }
}

fn device() -> Device {
    Device {
        id: "D1".to_owned(),
        info: DeviceInfo {
            name: "Stream Deck".to_owned(),
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
        action: format!("{UUID}.say"),
        context: context.to_owned(),
        device: "D1".to_owned(),
        payload: ActionPayload {
            settings: Settings::new(),
            coordinates: Some(Coordinates { column: 0, row: 0 }),
            state: None,
            user_desired_state: None,
            resources: Resources::new(),
        },
    })
}

async fn recv(host: &mut PluginHost) -> Request {
    timeout(WAIT, host.recv())
        .await
        .expect("a request")
        .expect("the host runs")
}

async fn next_json(socket: &mut Socket) -> Value {
    loop {
        match timeout(WAIT, socket.next()).await.expect("a message") {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(_)) => {}
            other => panic!("connection ended: {other:?}"),
        }
    }
}

async fn send_json(socket: &mut Socket, value: Value) {
    socket.send(Message::text(value.to_string())).await.unwrap();
}

#[tokio::test]
async fn manual_plugin() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(temp.path(), "");
    let mut host = PluginHost::start(config(temp.path(), plugin, Launch::Manual))
        .await
        .unwrap();
    host.device_connected(device());
    let launch = host.launch_manually(UUID).await.unwrap();
    assert_eq!(launch.port, host.port());
    let info: Value = serde_json::from_str(&launch.info).unwrap();
    assert_eq!(info["devices"][0]["id"], "D1");
    assert_eq!(info["application"]["version"], "7.1.0");

    let url = format!("ws://127.0.0.1:{}", host.port());
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    send_json(
        &mut socket,
        json!({ "event": "registerPlugin", "uuid": launch.token }),
    )
    .await;
    assert_eq!(next_json(&mut socket).await["event"], "deviceDidConnect");
    assert_eq!(
        recv(&mut host).await,
        Request::Registered {
            plugin: UUID.to_owned()
        }
    );

    host.send(UUID, will_appear("ctx"));
    assert_eq!(next_json(&mut socket).await["context"], "ctx");
    send_json(
        &mut socket,
        json!({ "event": "setSettings", "context": "ctx", "payload": { "said": 1 } }),
    )
    .await;
    let Request::SetSettings { context, from, .. } = recv(&mut host).await else {
        panic!("expected setSettings");
    };
    assert_eq!((context.as_str(), from), ("ctx", Sender::Plugin));

    // Global settings survive the host.
    send_json(
        &mut socket,
        json!({ "event": "setGlobalSettings", "context": launch.token, "payload": { "g": true } }),
    )
    .await;
    send_json(
        &mut socket,
        json!({ "event": "getGlobalSettings", "context": launch.token, "id": "1" }),
    )
    .await;
    assert_eq!(
        next_json(&mut socket).await,
        json!({ "event": "didReceiveGlobalSettings", "payload": { "settings": { "g": true } }, "id": "1" })
    );
    host.shutdown().await;
    let saved = fs::read_to_string(
        temp.path()
            .join(format!("config/plugins/{UUID}/settings.json")),
    )
    .unwrap();
    assert!(saved.contains("\"g\": true"), "{saved}");
}

#[tokio::test]
async fn web_pages_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(temp.path(), "");
    let host = PluginHost::start(config(temp.path(), plugin, Launch::Manual))
        .await
        .unwrap();
    let url = format!("ws://127.0.0.1:{}", host.port());
    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_err());

    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "superdeck://inspector".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_ok());

    // Unregistered connections are closed.
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    send_json(
        &mut socket,
        json!({ "event": "registerPlugin", "uuid": "guess" }),
    )
    .await;
    let end = timeout(WAIT, socket.next()).await.unwrap();
    assert!(
        matches!(end, None | Some(Ok(Message::Close(_)) | Err(_))),
        "{end:?}"
    );
    host.shutdown().await;
}

/// A plugin on Node.js that answers `willAppear` with `setTitle`, and exits
/// on `keyDown` so it's started again.
const ECHO: &str = r#"
const args = process.argv.slice(2);
const arg = (name) => args[args.indexOf(name) + 1];
const info = JSON.parse(arg("-info"));
const ws = new WebSocket(`ws://127.0.0.1:${arg("-port")}`);
ws.onopen = () => {
  ws.send(JSON.stringify({ event: arg("-registerEvent"), uuid: arg("-pluginUUID") }));
  ws.send(JSON.stringify({ event: "logMessage", payload: { message: `cwd ${process.cwd()} platform ${info.application.platform}` } }));
};
ws.onmessage = (message) => {
  const event = JSON.parse(message.data);
  if (event.event === "willAppear") {
    ws.send(JSON.stringify({ event: "setTitle", context: event.context, payload: { title: "hello" } }));
  } else if (event.event === "keyDown") {
    console.error("exiting on request");
    process.exit(3);
  }
};
"#;

#[tokio::test]
async fn node_plugin_is_supervised() {
    let Ok(node) = NodeRuntime::find() else {
        eprintln!("skipped: no Node.js 24 on PATH");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(temp.path(), ECHO);
    let dir = plugin.dir.clone();
    let mut host = PluginHost::start(config(temp.path(), plugin, Launch::Node(node)))
        .await
        .unwrap();

    for run in 0..2 {
        loop {
            match recv(&mut host).await {
                Request::Registered { .. } => break,
                Request::Started { .. }
                | Request::Stopped { .. }
                | Request::Disconnected { .. } => {}
                other => panic!("run {run}: unexpected {other:?}"),
            }
        }
        host.send(UUID, will_appear("ctx"));
        assert_eq!(
            recv(&mut host).await,
            Request::SetTitle {
                plugin: UUID.to_owned(),
                context: "ctx".to_owned(),
                state: None,
                title: Some("hello".to_owned()),
            }
        );
        host.send(
            UUID,
            Event::KeyDown(ActionEvent {
                action: format!("{UUID}.say"),
                context: "ctx".to_owned(),
                device: "D1".to_owned(),
                payload: ActionPayload {
                    settings: Settings::new(),
                    coordinates: Some(Coordinates { column: 0, row: 0 }),
                    state: None,
                    user_desired_state: None,
                    resources: Resources::new(),
                },
            }),
        );
    }
    host.shutdown().await;

    let log = fs::read_to_string(temp.path().join(format!("logs/{UUID}.log"))).unwrap();
    for expected in [
        "asks for Stream Deck 7.4",
        &format!("[plugin] cwd {} platform linux", dir.display()),
        "[stderr] exiting on request",
        "exited (exit status: 3); starting again in 1 s",
        "stopped",
    ] {
        assert!(log.contains(expected), "no {expected:?} in:\n{log}");
    }
}
