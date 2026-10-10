// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebSocket server, and the task that owns the [`Router`]: every
//! connection, plugin process, and call from the app sends it an
//! [`Input`], and it carries out what the router says.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HttpRequest, Response as HttpResponse,
};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::message::{ActionPayload, Device, Event, InspectorEvent};
use crate::router::{ConnId, InspectorLaunch, Output, PluginLaunch, Request, Router};

/// Messages are limited to this size.
pub(crate) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// A connection must register within this time.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Something for the router task.
#[derive(Debug)]
pub(crate) enum Input {
    /// A plugin process is about to start; the reply has its arguments.
    Launch {
        plugin: String,
        reply: oneshot::Sender<Option<PluginLaunch>>,
    },
    /// A plugin process ended.
    Stopped {
        plugin: String,
    },
    /// A note for the app about a plugin process.
    Status(Request),
    Opened {
        conn: ConnId,
        outgoing: mpsc::UnboundedSender<Outgoing>,
    },
    Received {
        conn: ConnId,
        text: String,
    },
    Closed {
        conn: ConnId,
    },
    Send {
        plugin: String,
        event: Event,
    },
    SendToInspector {
        context: String,
        event: InspectorEvent,
    },
    Broadcast(Event),
    DeviceConnected(Device),
    DeviceDisconnected(String),
    OpenInspector {
        plugin: String,
        action: String,
        context: String,
        device: String,
        payload: ActionPayload,
        reply: oneshot::Sender<Option<InspectorLaunch>>,
    },
    CloseInspector {
        context: String,
    },
}

/// Something for a connection's writer.
#[derive(Debug)]
pub(crate) enum Outgoing {
    Text(String),
    Close,
}

/// Runs the router until every sender of inputs is gone.
pub(crate) async fn route(
    mut router: Router,
    mut inputs: mpsc::UnboundedReceiver<Input>,
    requests: mpsc::UnboundedSender<Request>,
) {
    let mut connections: HashMap<ConnId, mpsc::UnboundedSender<Outgoing>> = HashMap::new();
    while let Some(input) = inputs.recv().await {
        let outputs = match input {
            Input::Launch { plugin, reply } => {
                let (launch, outputs) = router.launch(&plugin).unzip();
                let _ = reply.send(launch);
                outputs.unwrap_or_default()
            }
            Input::Stopped { plugin } => router.stopped(&plugin),
            Input::Status(request) => vec![Output::Request(request)],
            Input::Opened { conn, outgoing } => {
                connections.insert(conn, outgoing);
                router.opened(conn);
                Vec::new()
            }
            Input::Received { conn, text } => router.received(conn, &text),
            Input::Closed { conn } => {
                connections.remove(&conn);
                router.closed(conn)
            }
            Input::Send { plugin, event } => router.send(&plugin, &event),
            Input::SendToInspector { context, event } => router.send_to_inspector(&context, &event),
            Input::Broadcast(event) => router.broadcast(&event),
            Input::DeviceConnected(device) => router.device_connected(device),
            Input::DeviceDisconnected(id) => router.device_disconnected(&id),
            Input::OpenInspector {
                plugin,
                action,
                context,
                device,
                payload,
                reply,
            } => {
                let (launch, outputs) = router
                    .open_inspector(&plugin, &action, &context, &device, &payload)
                    .unzip();
                let _ = reply.send(launch);
                outputs.unwrap_or_default()
            }
            Input::CloseInspector { context } => router.close_inspector(&context),
        };
        for output in outputs {
            match output {
                Output::Send(conn, text) => {
                    if let Some(outgoing) = connections.get(&conn) {
                        let _ = outgoing.send(Outgoing::Text(text));
                    }
                }
                Output::Close(conn) => {
                    if let Some(outgoing) = connections.remove(&conn) {
                        let _ = outgoing.send(Outgoing::Close);
                    }
                }
                Output::Request(request) => {
                    let _ = requests.send(request);
                }
            }
        }
    }
}

/// Accepts connections until the router task is gone.
pub(crate) async fn accept(
    listener: TcpListener,
    inputs: mpsc::UnboundedSender<Input>,
    origins: Arc<[String]>,
) {
    let mut next: ConnId = 0;
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        if inputs.is_closed() {
            return;
        }
        next += 1;
        tokio::spawn(connection(
            stream,
            next,
            inputs.clone(),
            Arc::clone(&origins),
        ));
    }
}

async fn connection(
    stream: TcpStream,
    conn: ConnId,
    inputs: mpsc::UnboundedSender<Input>,
    origins: Arc<[String]>,
) {
    let _ = stream.set_nodelay(true);
    // Web pages may connect to local servers; only the app's own origin
    // is let in. Plugins on Node.js send no Origin.
    #[expect(clippy::result_large_err, reason = "tungstenite's callback signature")]
    let check_origin =
        |request: &HttpRequest, response: HttpResponse| match request.headers().get("origin") {
            Some(origin)
                if !origins
                    .iter()
                    .any(|allowed| origin.as_bytes() == allowed.as_bytes()) =>
            {
                let mut refusal = ErrorResponse::new(Some("origin not allowed".to_owned()));
                *refusal.status_mut() = StatusCode::FORBIDDEN;
                Err(refusal)
            }
            _ => Ok(response),
        };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let Ok(socket) =
        tokio_tungstenite::accept_hdr_async_with_config(stream, check_origin, Some(config)).await
    else {
        return;
    };
    let (mut write, mut read) = socket.split();
    let (outgoing, mut queue) = mpsc::unbounded_channel();
    if inputs.send(Input::Opened { conn, outgoing }).is_err() {
        return;
    }

    let mut writer = tokio::spawn(async move {
        while let Some(message) = queue.recv().await {
            match message {
                Outgoing::Text(text) => {
                    if write.send(Message::text(text)).await.is_err() {
                        break;
                    }
                }
                Outgoing::Close => break,
            }
        }
        let _ = write.close().await;
    });

    let deadline = tokio::time::Instant::now() + REGISTRATION_TIMEOUT;
    let mut registered = false;
    loop {
        let next = tokio::select! {
            next = read.next() => next,
            // The router closed the connection.
            _ = &mut writer => break,
            () = tokio::time::sleep_until(deadline), if !registered => break,
        };
        let text = match next {
            Some(Ok(Message::Text(text))) => text.as_str().to_owned(),
            // Some clients send JSON as binary frames.
            Some(Ok(Message::Binary(data))) => match String::from_utf8(data.to_vec()) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            Some(Ok(_)) => continue,
        };
        registered = true;
        if inputs.send(Input::Received { conn, text }).is_err() {
            break;
        }
    }
    let _ = inputs.send(Input::Closed { conn });
    writer.abort();
}
