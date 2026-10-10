# Plugin protocol

> [!NOTE]
> **Status: draft for milestone M3.** It changes as `superdeck-plugin-host`
> and `@superdeck/sdk` are written.

This is how SuperDeck runs plugins and talks to them. It is the protocol of
the Stream Deck SDK, so existing plugins run unchanged, and this document
only spells out the parts the SDK documentation leaves to the host, and the
places where SuperDeck behaves differently.

The JSON Schemas in [`schemas/`](schemas/) are the contract. The plugin
host's Rust types and `@superdeck/sdk`'s TypeScript types are tested against
them, so a change to the protocol starts with a change to a schema.

| Schema                                                                     | What it describes                                  |
| -------------------------------------------------------------------------- | -------------------------------------------------- |
| [`manifest.schema.json`](schemas/manifest.schema.json)                     | A plugin's `manifest.json`, as SuperDeck reads it  |
| [`registration-info.schema.json`](schemas/registration-info.schema.json)   | The `-info` argument, and `inInfo` of an inspector |
| [`action-info.schema.json`](schemas/action-info.schema.json)               | `inActionInfo`, given to a property inspector      |
| [`registration.schema.json`](schemas/registration.schema.json)             | The first message on a connection                  |
| [`plugin-events.schema.json`](schemas/plugin-events.schema.json)           | Messages from SuperDeck to a plugin                |
| [`plugin-commands.schema.json`](schemas/plugin-commands.schema.json)       | Messages from a plugin to SuperDeck                |
| [`inspector-events.schema.json`](schemas/inspector-events.schema.json)     | Messages from SuperDeck to a property inspector    |
| [`inspector-commands.schema.json`](schemas/inspector-commands.schema.json) | Messages from a property inspector to SuperDeck    |
| [`common.schema.json`](schemas/common.schema.json)                         | Definitions the others share                       |

## Compatibility

SuperDeck implements the protocol of **Stream Deck 7.1** for key devices,
and reports that as its version (`application.version` is `"7.1.0"`). The
official SDK checks that number before using newer features, such as the
message IDs of `getSettings`, so it's the version of the official app whose
key-device protocol SuperDeck has caught up with, not SuperDeck's own
version.

- Plugins are run whatever their `SDKVersion` (2 or 3).
- A plugin whose `Software.MinimumVersion` is newer than 7.1 is still
  started, with a warning in its log: the official SDK checks for no
  key-device feature newer than 7.1.
- Stream Deck + dials and touch strips, the Neo info bar, pedals, and
  key logic actions come after 1.0. Their commands are accepted and ignored,
  and their events are never sent.

## Plugins on disk

A plugin is a directory named `<uuid>.sdPlugin` holding a `manifest.json`.
SuperDeck looks for plugins in two places:

| Where                               | What                           |
| ----------------------------------- | ------------------------------ |
| `<prefix>/share/superdeck/plugins/` | Plugins bundled with SuperDeck |
| `$XDG_DATA_HOME/superdeck/plugins/` | Plugins the user installed     |

A user-installed plugin with the same UUID as a bundled one replaces it, so
a newer version can be installed over a bundled plugin.

### The manifest

SuperDeck reads the fields of the official manifest that apply on Linux, and
is lenient about the rest, because manifests are written for the official
app:

- **`UUID`** identifies the plugin. It must match the directory name
  without `.sdPlugin`. Older manifests without a `UUID` take it from the
  directory name.
- **`Actions`** need a `UUID`, a `Name`, and one or two `States`. Action
  UUIDs must be unique within the plugin.
- **`OS`** lists macOS and Windows, never Linux. SuperDeck ignores it, and
  the actions' `OS` lists too.
- **`CodePath`** is the entry point. `CodePathMac` and `CodePathWin` are
  ignored. See [Running plugins](#running-plugins) for which entry points
  can run.
- **`ApplicationsToMonitor`** lists macOS bundle IDs and Windows executable
  names, which mean nothing on Linux, so `applicationDidLaunch` and
  `applicationDidTerminate` are never sent. Smart profiles (milestone M6)
  may change that.
- Unknown fields are ignored, not rejected.

A manifest that can't be read, or lacks a required field, disables that
plugin only, with the reason in the log.

Images named in the manifest (`Icon`, `States[].Image`, and so on) leave out
the file extension. SuperDeck looks for `<path>.svg`, `<path>@2x.png`,
`<path>.png`, and `<path>.gif`, in that order, and uses the first that
exists.

## Running plugins

A plugin runs as a child process of SuperDeck, as the user, unsandboxed, as
in the official app.

**Node.js plugins** are those with a `Nodejs` section in the manifest and a
`CodePath` ending in `.js`, `.mjs`, or `.cjs`. They all run on Node.js 24
or newer, whatever `Nodejs.Version` asks for: the runtime bundled with the
Flatpak and AppImage, or the system `node` for the other packages. SuperDeck
checks the version before starting any plugin, and starts none if it's too
old.

The command line is:

```sh
node --enable-source-maps --no-global-search-paths <CodePath> \
  -port <port> -pluginUUID <token> -registerEvent registerPlugin -info <json>
```

- The working directory is the plugin directory, since plugins read their
  `manifest.json` and translations from there.
- `<token>` is a random UUID, new for every start. The plugin sends it back
  to register, and it's the `context` of the plugin's global-settings
  commands. It is not the plugin's UUID.
- `<json>` is the [registration info](#registration-info).
- `Nodejs.Debug` is honored only when SuperDeck runs with plugin debugging
  turned on: `"enabled"` adds `--inspect=127.0.0.1:<port>` and `"break"`
  adds `--inspect-brk=127.0.0.1:<port>`, with a free port that is written to
  the plugin's log. Other values are passed to Node.js as they are.

**Other plugins don't run yet.** Plugins whose `CodePath` is an HTML page
run in a browser engine in the official app; SuperDeck will run them in a
hidden web view of the desktop app (milestone M4). Native executables for
macOS and Windows can't run on Linux. These plugins are listed with the
reason they're not running.

### Supervision

When a plugin exits, for any reason, SuperDeck starts it again after a
delay: one second, then twice as long after each exit, up to a minute. The
delay goes back to one second once the plugin has been running for a
minute. Plugins are stopped when SuperDeck quits.

### Logs

Each plugin has a log file, `$XDG_STATE_HOME/superdeck/logs/plugins/<uuid>.log`.
It gets what the plugin writes to its standard output and standard error,
the messages it sends with `logMessage`, and SuperDeck's notes about it:
starts, exits, and messages it refused. The previous run's log is kept as
`<uuid>.log.1`, and a log stops growing at 10 MiB.

## Connections

SuperDeck runs one WebSocket server for plugins and property inspectors,
bound to `127.0.0.1` on a random port. Messages are JSON text frames, one
message per frame.

### Registration

The first message on a connection registers it:

```json
{ "event": "registerPlugin", "uuid": "<token>" }
```

A property inspector sends `"registerPropertyInspector"` with the UUID it
was given. SuperDeck closes a connection that doesn't register within ten
seconds, or registers with a UUID it didn't hand out, or a second time.

When a plugin registers, SuperDeck sends it `deviceDidConnect` for every
connected device, then `willAppear` for each of its actions that is showing.
Events for a plugin that isn't connected are dropped, not queued: a plugin
that starts, or starts again, learns what's showing from these
`willAppear`s.

### Registration info

The `-info` argument, and a property inspector's `inInfo`, is a JSON object:

```json
{
  "application": {
    "font": "Noto Sans",
    "language": "en",
    "platform": "linux",
    "platformVersion": "6.12.4",
    "version": "7.1.0"
  },
  "plugin": { "uuid": "com.example.counter", "version": "1.0.0.0" },
  "devicePixelRatio": 1,
  "colors": {
    "buttonMouseOverBackgroundColor": "#464646",
    "buttonPressedBackgroundColor": "#303030",
    "buttonPressedBorderColor": "#646464",
    "buttonPressedTextColor": "#969696",
    "highlightColor": "#0078ff"
  },
  "devices": [
    {
      "id": "6B8F0E25A31C4D7E9F2B8A6C5D4E3F21",
      "name": "Stream Deck MK.2",
      "size": { "columns": 5, "rows": 3 },
      "type": 0
    }
  ]
}
```

- `platform` is `"linux"`. The official app only ever says `"mac"` or
  `"windows"`; the official SDK passes the value through without checking
  it.
- `platformVersion` is the kernel release.
- `language` is the user's language if the official app supports it (`de`,
  `en`, `es`, `fr`, `ja`, `ko`, `zh_CN`, `zh_TW`), otherwise `en`.
- `devices` are the devices connected when the plugin started. A device ID
  is the first 16 bytes of the SHA-256 hash of its serial number, in
  uppercase hexadecimal: stable, and not the serial number itself.
- Device `type` follows the SDK's numbers: 0 for the Original, MK.2, and
  Module 15; 1 for the Mini and Module 6; 2 for the XL and Module 32; 9 for
  the Neo.

## Events and commands

The messages are those of the Stream Deck SDK. The schemas give every field;
this section says how SuperDeck uses them.

### Events to plugins

| Event                                                         | When SuperDeck sends it                                                        |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `deviceDidConnect`, `deviceDidDisconnect`                     | A device is plugged in or unplugged, and when the plugin registers             |
| `willAppear`, `willDisappear`                                 | One of the plugin's actions starts or stops showing on a device                |
| `keyDown`, `keyUp`                                            | A key is pressed or released, or a Multi Action runs the action                |
| `didReceiveSettings`                                          | Answer to `getSettings`, or the property inspector saved settings              |
| `didReceiveGlobalSettings`                                    | Answer to `getGlobalSettings`, or the property inspector saved global settings |
| `didReceiveResources`                                         | Answer to `getResources`, or the property inspector saved resources            |
| `titleParametersDidChange`                                    | After `willAppear`, and when the user changes the title or its style           |
| `propertyInspectorDidAppear`, `propertyInspectorDidDisappear` | The action's property inspector opens or closes                                |
| `sendToPlugin`                                                | The property inspector sent the plugin a message                               |
| `systemDidWakeUp`                                             | The computer resumed from suspend                                              |

Never sent: `applicationDidLaunch` and `applicationDidTerminate` (see
[the manifest](#the-manifest)), `didReceiveDeepLink` (SuperDeck has no
`streamdeck://` URL handler), `deviceDidChange` (devices can't be renamed
yet), and the Stream Deck + events `dialDown`, `dialUp`, `dialRotate`, and
`touchTap`.

A `keyDown` or `keyUp` from a Multi Action has `isInMultiAction: true` and
no `coordinates`. SuperDeck runs a Multi Action step as a `keyDown`
followed straight away by a `keyUp`.

A two-state action switches to its other state after its `keyUp` is sent,
unless the manifest sets `DisableAutomaticStates`, so `keyDown` and `keyUp`
both carry the state the key had when it was pressed.

### Commands from plugins

| Command                        | What SuperDeck does                                                                        |
| ------------------------------ | ------------------------------------------------------------------------------------------ |
| `setSettings`                  | Saves the instance's settings and sends `didReceiveSettings` to its property inspector     |
| `getSettings`                  | Answers with `didReceiveSettings`, echoing `id`                                            |
| `setGlobalSettings`            | Saves the plugin's settings and sends `didReceiveGlobalSettings` to its property inspector |
| `getGlobalSettings`            | Answers with `didReceiveGlobalSettings`, echoing `id`                                      |
| `setResources`, `getResources` | As for settings                                                                            |
| `setTitle`, `setImage`         | Changes what the key shows, for one state or both; leaving out the value resets it         |
| `setState`                     | Changes the instance's state (0 or 1)                                                      |
| `showAlert`, `showOk`          | Shows a warning or check mark on the key for a moment                                      |
| `switchToProfile`              | Switches the device to a profile the plugin ships (once they can be installed, in M6)      |
| `sendToPropertyInspector`      | Passes the payload to the action's property inspector, if it's open                        |
| `openUrl`                      | Opens the URL in the default browser, through the XDG portal or `xdg-open`                 |
| `logMessage`                   | Writes the message to the plugin's log                                                     |

- `setImage` takes a `data:` URL (base64 PNG, JPEG, BMP, GIF, or SVG, or
  SVG text as `data:image/svg+xml,<svg …>`), or a path to an image inside
  the plugin directory. A path that leads outside the plugin directory is
  refused.
- `target` (hardware, software, or both) of `setTitle` and `setImage` is
  ignored: the key and its picture in the app always match.
- Accepted and ignored: `setFeedback`, `setFeedbackLayout`, and
  `setTriggerDescription` (Stream Deck +).
- Unknown commands are logged and ignored.

### Property inspectors

A property inspector is the HTML page an action names in
`PropertyInspectorPath`, shown in the main window when the user selects a
key (milestone M4). It connects as in the official app: SuperDeck calls its
`connectElgatoStreamDeckSocket(port, uuid, registerEvent, info, actionInfo)`,
where `uuid` is the action instance's context, `registerEvent` is
`"registerPropertyInspector"`, `info` is the
[registration info](#registration-info) and `actionInfo` describes the
instance.

It may send `setSettings`, `getSettings`, `setGlobalSettings`,
`getGlobalSettings`, `setResources`, `getResources`, `sendToPlugin`,
`openUrl`, and `logMessage`, and it receives `didReceiveSettings`,
`didReceiveGlobalSettings`, `didReceiveResources`, and
`sendToPropertyInspector`. A setting it saves goes to the plugin as
`didReceiveSettings`, and one the plugin saves comes to it the same way.

## Settings

- **Instance settings** belong to the action instance and are stored with
  it in its profile; see
  [the profile format](../../docs/profile-format.md#action-instances).
- **Global settings** belong to the plugin, and are stored in
  `$XDG_CONFIG_HOME/superdeck/plugins/<uuid>/settings.json`:

  ```json
  { "formatVersion": 1, "settings": { "apiKey": "…" } }
  ```

  The file is replaced atomically, follows the conventions of the profile
  format, and is readable only by the user, since plugins keep secrets such
  as API tokens there.

- **Resources** are files an instance refers to, as a map from names to
  paths. They are stored with the instance, like its settings.

Settings are JSON objects, kept exactly as given.

## Security

The official app trusts any program that connects to it. SuperDeck is
stricter:

- The server listens on the loopback interface only.
- A connection must register with a UUID that SuperDeck handed out: a
  plugin's start token, or the context of a property inspector SuperDeck
  has opened.
- Web pages can connect to local WebSocket servers. SuperDeck refuses a
  connection that has an `Origin` header, unless it is the desktop app's own
  origin for property inspectors. Plugins run on Node.js and send no
  `Origin`.
- A plugin may only address the contexts of its own actions, and a property
  inspector only its own instance. Messages addressing other contexts are
  dropped and logged.
- Messages are limited to 16 MiB, enough for an animated GIF as a `data:`
  URL.

Plugins still run as the user with the user's rights, as on Windows, so
users should only install plugins they trust.

## Extensions

SuperDeck has no protocol extensions yet. When a plugin needs something the
SDK can't do, such as sending a hotkey on Wayland (milestone M5), it will be
added here: events and commands named `superdeck.<name>`, with a schema, so
that third-party plugins can use it too, and plugins can tell whether they
are running on SuperDeck before using one.
