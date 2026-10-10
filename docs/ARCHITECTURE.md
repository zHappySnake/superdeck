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

Talks to the hardware over `hidraw`, via `hidapi` or the `elgato-streamdeck`
crate (to be decided in M1). It knows each model's key layout, image size and
orientation, image format, and report layout. It produces a stream of input
events (key down/up, later dial/touch) and accepts output commands (set key
image, brightness, reset). It has no knowledge of profiles or plugins.

### `crates/superdeck-render`

Turns an action's visual state into the exact bitmap a device key needs: the
plugin-supplied image (PNG, JPEG, SVG, GIF frame, or data URL) plus the title
overlay drawn with the title settings. Rendering is pure and deterministic, so
it can be snapshot-tested without hardware.

### `crates/superdeck-core`

The application model: devices → profiles → pages → keys → action instances,
plus folders, Multi Actions, and the core navigation actions. It owns
persistence, undo/redo, smart profile switching, and dispatching input events
to the core or to plugins. It exposes an async API that both the Tauri app and
a headless mode can drive.

### `crates/superdeck-plugin-host`

Implements the Stream Deck SDK host side:

- discovers plugins (bundled, then user-installed) and validates `manifest.json`
- spawns each plugin with the documented command-line arguments (`-port`,
  `-pluginUUID`, `-registerEvent`, `-info`), using the shipped Node.js
  runtime for `CodePath` JavaScript entries
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
