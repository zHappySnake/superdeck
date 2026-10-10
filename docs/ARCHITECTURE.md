# Architecture

This describes the planned design. It will change as milestones land. The
roadmap explains _what_ and _when_; this file explains _how_.

## Overview

```
┌──────────────────────────── superdeck (one process) ────────────────────────────┐
│                                                                                 │
│   ┌──────────── UI (app/src) ───────────┐     ┌──── Tray (StatusNotifierItem) ┐ │
│   │ Svelte 5 + TS in Tauri webview      │     └───────────────────────────────┘ │
│   │  ├─ device canvas / action list     │                                       │
│   │  └─ property inspector <iframe>s ───┼───────────────┐ ws (PI registration)  │
│   └──────────────┬──────────────────────┘               │                       │
│                  │ Tauri commands + events              │                       │
│   ┌──────────────▼──────────── Core (crates/) ──────────┼──────────────────┐    │
│   │ superdeck-core     profiles · pages · folders · actions · persistence │    │
│   │ superdeck-render   key image composition (images, SVG, GIF, titles)   │    │
│   │ superdeck-device   USB HID drivers per model, hotplug ── hidraw ──────┼──► Stream Deck
│   │ superdeck-plugin-host  WebSocket server · plugin processes ◄──────────┘    │
│   └──────────────────────────────────┬─────────────────────────────────────┘    │
└──────────────────────────────────────┼──────────────────────────────────────────┘
                                       │ ws://127.0.0.1:<random port>
                 ┌─────────────────────┼──────────────────────┐
           node plugins/system   node plugins/soundboard   third-party .sdPlugin
```

## Components

### `crates/superdeck-device`

Our own implementation of the Stream Deck USB HID protocol, documented in
[device-protocol.md](device-protocol.md). It has no knowledge of profiles or
plugins.

- `Model` and its `Spec` describe each device: key layout, image size,
  orientation and format, and which of the three protocol generations it
  speaks.
- `protocol` turns commands into report bytes and report bytes into key
  states. It's pure: no I/O.
- `Transport` is the only I/O boundary: write, feature report get/set,
  read. `HidTransport` is the real one, on the `hidapi` crate (hidraw
  backend). `Recorder` wraps a transport and writes a text capture of the
  traffic; `Replay` plays a capture back as a fake device, which is how the
  protocol is tested without hardware.
- `Device` is an open device: key images (any image, scaled, oriented and
  encoded per model, or pre-encoded bytes for callers that cache), brightness,
  reset, serial number and firmware queries, and key down/up events derived
  from successive key state reports. Calls block; a `Device` is owned by one
  thread at a time, which interleaves short input reads with output commands.
- `DeviceManager` lists supported devices, opens them, and reports
  connects and disconnects. It listens for hidraw events from a `udev`
  monitor and rescans with hidapi on each one, falling back to rescanning
  every second when udev events aren't available.

### `crates/superdeck-cli`

A small binary for driving a device by hand (`list`, `info`, `watch`,
`events`, `brightness`, `fill`, `image`, `render`, `test-pattern`, `clear`,
`reset`), and for recording captures with `--record`. `render` draws a key
with `superdeck-render`, on a device or into a PNG file. Not shipped to users.

### `crates/superdeck-render`

Turns a key's image and title into the bitmap the key shows. It knows
nothing about devices, profiles, or plugins.

- A `Renderer` holds the fonts and is created once. It loads a `KeyImage`
  from bytes or a `data:` URL (as sent by `setImage`), recognizing PNG, JPEG,
  BMP, still or animated GIF, and SVG from the content rather than the
  declared type. Loaded images are kept premultiplied and scaled down to at
  most 288 px, so they can be rendered at any key size cheaply. Decoding has
  size limits, and SVGs can't reference files.
- `render(image, title, style, size)` draws the image scaled to fit and
  centered on black, then the title, and returns an upright RGB image, or
  one per frame with its delay for an animation. `superdeck-device` then
  scales, rotates, and encodes it for the model.
- Titles follow the SDK's title parameters (font, size, bold, italic,
  underline, top/middle/bottom, color, shown or hidden). Text is shaped and
  laid out with `cosmic-text`; lines are centered and not wrapped, and get a
  thin dark outline so they stay readable on any image. Font sizes are
  pixels on a 72 px key and scale with the key.
- The default font is a bundled copy of Noto Sans (OFL-1.1), so keys look
  identical on every distribution; users can pick any installed font. SVG
  text uses the same font database, through `resvg`.

Rendering with only the bundled fonts is deterministic, so it is tested
against snapshot images without hardware. The title metrics (size unit,
margins, outline) are our own choices and still need comparing with the
official app.

### `crates/superdeck-core`

The application model: devices → profiles → pages → keys → action instances,
plus folders, Multi Actions, and the core navigation actions. It owns
persistence (our own versioned JSON format with migrations, described in
[profile-format.md](profile-format.md); the official `.streamDeckProfile`
format is handled only by import/export converters),
undo/redo, smart profile switching, and dispatching input events
to the core or to plugins. It exposes an async API that both the Tauri app and
a headless mode can drive.

### `crates/superdeck-plugin-host`

Implements the Stream Deck SDK host side:

- discovers plugins (bundled, then user-installed) and validates `manifest.json`
- spawns each plugin with the documented command-line arguments (`-port`,
  `-pluginUUID`, `-registerEvent`, `-info`). JavaScript `CodePath` entries
  all run on Node.js 24 LTS, whatever version the manifest requests: the
  bundled runtime in Flatpak/AppImage, or the system `nodejs` (24 or newer,
  checked before starting plugins) in native packages
- runs one WebSocket server on `127.0.0.1` at a random port, accepts
  registrations from plugins and property inspectors, and routes events and
  commands between them and the core
- supervises processes (restart with backoff, log capture to
  `$XDG_STATE_HOME/superdeck/logs/plugins/`)

### `app/` (Tauri)

`app/src-tauri` is a thin Rust shell. It starts the core, owns the window and
tray, and maps core state to Tauri commands and events. `app/src` is the Svelte
UI that reproduces the official app's layout. Property inspectors load in
sandboxed iframes served from a custom URI scheme and connect to the plugin
host like they do in the official app.

### `sdk/`

Everything a plugin author needs, under MIT OR Apache-2.0:

- `sdk/protocol/`: JSON Schemas for `manifest.json` and every WebSocket
  message. They are **the contract**: Rust types in the plugin host and
  TypeScript types in the SDK are generated from or tested against them.
  SuperDeck-specific extensions are namespaced and documented here.
- `sdk/js/`: `@superdeck/sdk`, a TypeScript library for writing plugins
  (connection, typed events, settings, property-inspector helpers), plus a
  mock host for unit testing plugins without the app.

### `plugins/`

Bundled plugins, each an independent npm package that builds a standard
`<id>.sdPlugin` directory. They depend only on `@superdeck/sdk`. CI builds and
tests them in isolation, and packaging copies their output into the app's data
directory. See [CONTRIBUTING.md § Plugins](../CONTRIBUTING.md#plugins) for the
rules.

Plugin IDs use the app's reverse-DNS prefix, for example
`io.github.zhappysnake.superdeck.system`.

## Dependency direction

```
app/src-tauri ──► superdeck-core ──► superdeck-render
                       │      └────► superdeck-device
                       └───────────► superdeck-plugin-host ──► sdk/protocol (schemas)

plugins/* ──► sdk/js ──► sdk/protocol          (never ──► app/ or crates/)
```

## Files on disk

| What                   | Where                                             |
| ---------------------- | ------------------------------------------------- |
| Settings, profiles     | `$XDG_CONFIG_HOME/superdeck/`                     |
| User-installed plugins | `$XDG_DATA_HOME/superdeck/plugins/<id>.sdPlugin/` |
| Icon packs             | `$XDG_DATA_HOME/superdeck/icon-packs/`            |
| Bundled plugins        | `<prefix>/share/superdeck/plugins/` (read-only)   |
| Logs                   | `$XDG_STATE_HOME/superdeck/logs/`                 |
| Caches (rendered keys) | `$XDG_CACHE_HOME/superdeck/`                      |

## Security model

- The WebSocket server binds to loopback only, on a random port.
- Each plugin and property inspector gets a unique registration UUID. The host
  drops connections that don't register with a known UUID, and drops messages
  that address contexts the sender doesn't own. This is stricter than the
  official app, which trusts any local client.
- Plugins run unsandboxed as the user, just as on Windows. Users should only
  install plugins they trust; the UI says so at install time.
- Property inspectors run in sandboxed iframes with no access to Tauri APIs.
