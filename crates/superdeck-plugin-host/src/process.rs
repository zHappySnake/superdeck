// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a plugin's process, and starting it again when it exits.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::info::COMPATIBLE_VERSION;
use crate::log::{PluginLog, Source};
use crate::manifest::{Entry, Plugin};
use crate::node::NodeRuntime;
use crate::router::Request;
use crate::server::Input;

/// Delay before the first restart.
const FIRST_DELAY: Duration = Duration::from_secs(1);

/// Longest delay between restarts.
const MAX_DELAY: Duration = Duration::from_mins(1);

/// A plugin that ran this long starts again after [`FIRST_DELAY`].
const STABLE: Duration = Duration::from_mins(1);

/// How long a plugin has to exit after `SIGTERM`.
const GRACE: Duration = Duration::from_secs(2);

/// How plugin processes are run.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub node: NodeRuntime,
    /// Honor `Nodejs.Debug`.
    pub debug: bool,
}

/// Runs a Node.js plugin until `shutdown` turns true, starting it again
/// whenever it exits.
pub(crate) async fn supervise(
    plugin: Arc<Plugin>,
    log: PluginLog,
    options: Options,
    inputs: mpsc::UnboundedSender<Input>,
    mut shutdown: watch::Receiver<bool>,
) {
    let Entry::Node { script, debug } = &plugin.entry else {
        return;
    };
    warn_if_too_new(&plugin, &log);
    let status = |request: Request| {
        let _ = inputs.send(Input::Status(request));
    };
    let mut delay = FIRST_DELAY;
    loop {
        let (reply, launch) = oneshot::channel();
        let input = Input::Launch {
            plugin: plugin.uuid.clone(),
            reply,
        };
        if inputs.send(input).is_err() {
            return;
        }
        let Ok(Some(launch)) = launch.await else {
            return;
        };

        let mut command = Command::new(&options.node.path);
        if options.debug
            && let Some(debug) = debug
        {
            command.args(debug_args(debug, &log));
        }
        command
            .arg("--enable-source-maps")
            .arg("--no-global-search-paths")
            .arg(script)
            .args(launch.args())
            .current_dir(&plugin.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        exit_with_host(&mut command);
        let started = Instant::now();
        let outcome = match command.spawn() {
            Ok(mut child) => {
                log.write(
                    Source::Host,
                    &format!(
                        "started {} on Node.js {} (process {})",
                        plugin.uuid,
                        options.node.version,
                        child.id().unwrap_or(0)
                    ),
                );
                status(Request::Started {
                    plugin: plugin.uuid.clone(),
                });
                capture(child.stdout.take(), Source::Stdout, &log);
                capture(child.stderr.take(), Source::Stderr, &log);
                tokio::select! {
                    exit = child.wait() => match exit {
                        Ok(exit) => format!("exited ({exit})"),
                        Err(e) => format!("could not be waited for: {e}"),
                    },
                    () = stopping(&mut shutdown) => {
                        stop(&mut child).await;
                        log.write(Source::Host, "stopped");
                        let _ = inputs.send(Input::Stopped { plugin: plugin.uuid.clone() });
                        return;
                    }
                }
            }
            Err(e) => format!("could not start: {e}"),
        };
        let _ = inputs.send(Input::Stopped {
            plugin: plugin.uuid.clone(),
        });
        delay = if started.elapsed() >= STABLE {
            FIRST_DELAY
        } else {
            delay
        };
        log.write(
            Source::Host,
            &format!("{outcome}; starting again in {} s", delay.as_secs()),
        );
        status(Request::Stopped {
            plugin: plugin.uuid.clone(),
            reason: outcome,
        });
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = stopping(&mut shutdown) => return,
        }
        delay = (delay * 2).min(MAX_DELAY);
    }
}

/// Has the kernel send the plugin `SIGTERM` if the thread that starts it
/// ends, so plugins don't outlive a host that crashed. Plugins are started
/// from the runtime's worker threads, which last as long as the host.
#[allow(unsafe_code)]
fn exit_with_host(command: &mut Command) {
    // SAFETY: the closure runs in the child between fork and exec, where
    // only async-signal-safe calls are allowed; it makes one prctl system
    // call and touches no memory shared with the parent.
    unsafe {
        command.pre_exec(|| {
            rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::TERM))
                .map_err(std::io::Error::from)
        });
    }
}

/// Waits until the host shuts down.
async fn stopping(shutdown: &mut watch::Receiver<bool>) {
    // An error means the host is gone, which is shutting down too.
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// Asks the plugin to exit, then makes it.
async fn stop(child: &mut Child) {
    if let Some(pid) = child.id().and_then(|id| i32::try_from(id).ok()) {
        if let Some(pid) = rustix::process::Pid::from_raw(pid) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        if tokio::time::timeout(GRACE, child.wait()).await.is_ok() {
            return;
        }
    }
    let _ = child.kill().await;
}

/// Copies a pipe to the log, line by line.
fn capture(pipe: Option<impl AsyncRead + Unpin + Send + 'static>, source: Source, log: &PluginLog) {
    let Some(pipe) = pipe else { return };
    let log = log.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log.write(source, &line);
        }
    });
}

/// Node.js arguments for `Nodejs.Debug`.
fn debug_args(debug: &str, log: &PluginLog) -> Vec<String> {
    let flag = match debug {
        "enabled" => "--inspect",
        "break" => "--inspect-brk",
        other => return other.split_whitespace().map(str::to_owned).collect(),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map_or(0, |addr| addr.port());
    log.write(Source::Host, &format!("debugger on 127.0.0.1:{port}"));
    vec![format!("{flag}=127.0.0.1:{port}")]
}

/// Notes in the log when a plugin asks for a newer official app than
/// SuperDeck's protocol matches.
fn warn_if_too_new(plugin: &Plugin, log: &PluginLog) {
    let Some(wanted) = plugin
        .manifest
        .software
        .as_ref()
        .and_then(|software| software.minimum_version.as_deref())
    else {
        return;
    };
    if version_parts(wanted) > version_parts(COMPATIBLE_VERSION) {
        log.write(
            Source::Host,
            &format!(
                "the plugin asks for Stream Deck {wanted}; SuperDeck implements the protocol of {COMPATIBLE_VERSION}, so some features may be missing"
            ),
        );
    }
}

fn version_parts(version: &str) -> [u32; 4] {
    let mut parts = [0; 4];
    for (part, value) in parts.iter_mut().zip(version.split('.')) {
        *part = value.trim().parse().unwrap_or(0);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(version_parts("7.2") > version_parts(COMPATIBLE_VERSION));
        assert!(version_parts("7.1") <= version_parts(COMPATIBLE_VERSION));
        assert!(version_parts("6.9.1") < version_parts("7"));
        assert!(version_parts("10.0") > version_parts("9.9.9.9"));
    }

    #[test]
    fn debug_arguments() {
        let log = PluginLog::discard();
        let [arg] = debug_args("enabled", &log).try_into().unwrap();
        assert!(arg.starts_with("--inspect=127.0.0.1:"), "{arg}");
        let [arg] = debug_args("break", &log).try_into().unwrap();
        assert!(arg.starts_with("--inspect-brk=127.0.0.1:"), "{arg}");
        assert_eq!(
            debug_args("--inspect=127.0.0.1:9229 --trace-warnings", &log),
            ["--inspect=127.0.0.1:9229", "--trace-warnings"]
        );
    }
}
