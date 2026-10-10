# Scope and roadmap

This document defines what SuperDeck is (and isn't), the feature set needed to
match the official Windows app, and the milestones that get us there. It's a
living document: open an issue to propose changes.

## Vision

Someone who uses Stream Deck on Windows should be able to plug the same device
into a Linux machine, open SuperDeck, and feel at home: same layout, same
actions, same plugins. Nothing phones home.

## Decisions

| Topic              | Decision                                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------- |
| Language (backend) | Rust (edition 2024), toolchain pinned in `rust-toolchain.toml`                                                                   |
| UI                 | Tauri v2 + Svelte 5 + TypeScript, rendered by the system WebKitGTK                                                               |
| Process model      | One process (core + UI + tray); plugins run as child processes                                                                   |
| Plugin API         | Compatible with the Stream Deck SDK (WebSocket protocol, `manifest.json`, HTML property inspectors)                              |
| Plugin runtime     | Node.js 24 LTS for every plugin, whatever `Nodejs.Version` the manifest requests; move to the next LTS when it's ready           |
| Node.js shipping   | Flatpak and AppImage bundle Node 24; `.deb`/`.rpm`/AUR/Nix depend on the system `nodejs` (>= 24), checked at startup             |
| Bundled plugins    | TypeScript packages in `plugins/`, depending only on `@superdeck/sdk`                                                            |
| Licensing          | App: GPL-3.0-or-later. `sdk/`: MIT OR Apache-2.0                                                                                 |
| Packaging          | Flatpak, AppImage, `.deb`, `.rpm`, AUR, Nix                                                                                      |
| v1 hardware        | Key-only devices (Original, MK.2, Mini, XL, Neo keys, Module)                                                                    |
| App name and ID    | SuperDeck, `io.github.zhappysnake.superdeck`                                                                                     |
| Device driver      | Our own protocol implementation in `superdeck-device`, on the `hidapi` crate (hidraw backend), with the `udev` crate for hotplug |
| Key title text     | `cosmic-text` (shaping, bidi, fallback); bundled Noto Sans (OFL-1.1) as the default font; any installed font selectable          |
| Profile storage    | Our own documented, versioned JSON format; `.streamDeckProfile` import/export through converters                                 |
| Contributions      | Inbound = outbound licensing, DCO sign-off (`git commit -s`) required, no CLA                                                    |

## Feature parity target

The official app's feature set, minus online features. Items marked _(post-1.0)_
are planned but not required for 1.0.

### Devices

- [ ] Detect devices automatically, with hotplug (connect/disconnect while running)
- [ ] Several devices at once, each with its own profiles
- [ ] Brightness control, and a sleep timeout that turns the screen off when idle
- [ ] Device info panel (model, serial number, firmware version)
- [ ] Offline preview of the device canvas when no device is connected
- [ ] _(post-1.0)_ Stream Deck + dials and touch strip; Pedal; Studio; Neo info bar and touch keys
- [ ] _(post-1.0)_ Virtual Stream Deck (on-screen deck)

### Main window (1:1 layout)

- [ ] Device selector, profile dropdown, and page controls in the header
- [ ] Key canvas laid out like the physical device, with drag & drop from the action list
- [ ] Searchable action list sidebar, grouped by plugin and category
- [ ] Property inspector pane under the canvas: title editor, state selector, icon picker, plugin HTML
- [ ] Title editor: text, font, size, style, alignment (top/middle/bottom), color, show/hide
- [ ] Custom key images: PNG, JPEG, SVG, and animated GIF; set from file or reset to default
- [ ] Copy, paste, cut, duplicate, delete, and undo/redo on keys
- [ ] Multi-state (toggle) actions, with an image and title for each state
- [ ] Drag actions between keys, pages, and folders
- [ ] Preferences window: general, profiles, plugins, devices
- [ ] System tray icon with a profile switcher; close to tray; start at login
- [ ] Light/dark appearance matching the official app
- [ ] Localization infrastructure (English first)

### Profiles, pages, folders

- [ ] Several profiles per device; create, rename, duplicate, delete, reorder
- [ ] Pages within a profile; next/previous/go-to page; page indicator
- [ ] Folders, nested to any depth, with a back key
- [ ] Smart profiles: switch profile automatically by the focused application (see [Linux challenges](#linux-specific-challenges))
- [ ] Import/export `.streamDeckProfile` files (interoperable with the official app where the actions exist)
- [ ] Local backup and restore

### Built-in actions

These match the official app's default action set (to be checked against the
current Windows release).

| Group       | Actions                                                                             | Where it lives       |
| ----------- | ----------------------------------------------------------------------------------- | -------------------- |
| Navigation  | Create Folder, Switch Profile, Next Page, Previous Page, Go to Page, Page Indicator | core                 |
| Stream Deck | Multi Action, Multi Action Switch, Brightness, Sleep                                | core                 |
| System      | Hotkey, Hotkey Switch, Open, Website, Multimedia, Text                              | `plugins/system`     |
| Soundboard  | Play Audio, Stop                                                                    | `plugins/soundboard` |

Actions in the core are the structural ones that contain or navigate other
actions, as in the official app. Everything else is a plugin.

### Plugins

- [ ] Stream Deck SDK WebSocket protocol: registration, all plugin and property-inspector events and commands
- [ ] `manifest.json` parsing and validation (SDK versions in use by current plugins)
- [ ] Node.js runtime management for `CodePath` plugins
- [ ] Property inspectors rendered inside the main window
- [ ] Install plugins from `.streamDeckPlugin` files and folders; enable, disable, and remove them
- [ ] Install icon packs from `.streamDeckIconPack` files
- [ ] Global and per-action settings persistence
- [ ] `@superdeck/sdk` TypeScript SDK and a plugin template
- [ ] Compatibility report: a tested list of popular open-source plugins

## Non-goals

| Not doing                                          | Why                                                              |
| -------------------------------------------------- | ---------------------------------------------------------------- |
| Marketplace, store, "Discover" tab                 | Online service; you install plugins and icon packs from files    |
| Elgato account, cloud backup, profile sync         | Online service                                                   |
| Built-in auto-updater                              | Your distribution or Flatpak handles updates                     |
| Telemetry or analytics                             | Privacy                                                          |
| Stream Deck Mobile pairing                         | Proprietary online pairing                                       |
| Device firmware updates                            | Needs Elgato's firmware files and a proprietary update protocol  |
| Windows / macOS builds                             | The official app already exists there                            |
| Windows- or macOS-only native plugins              | Can't run natively; a Wine-based experiment may come later       |
| Elgato app integrations (Wave Link, Camera Hub, …) | Proprietary desktop software; independent plugins may cover this |

## Linux-specific challenges

These need design work early because the official app doesn't face them.

| Problem                                | Plan                                                                                                                                          |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| HID permissions                        | udev rule with `TAG+="uaccess"` installed by packages; Flatpak docs explain the host-side rule                                                |
| Sending hotkeys and text on Wayland    | XDG RemoteDesktop portal / libei where available; `uinput` fallback; XTest on X11                                                             |
| Focused app detection (smart profiles) | Per-environment backends: X11 (EWMH), KDE (KWin script), GNOME (Shell extension), wlroots (`foreign-toplevel`), Hyprland/Sway IPC             |
| Media keys and volume                  | MPRIS over D-Bus; PipeWire (`wpctl`) / PulseAudio                                                                                             |
| Tray icon                              | StatusNotifierItem (GNOME needs the AppIndicator extension)                                                                                   |
| Autostart                              | XDG autostart; Background portal under Flatpak                                                                                                |
| Shipping Node.js for plugins           | Bundle Node 24 in Flatpak/AppImage; native packages depend on the distro `nodejs` and the host refuses to start plugins on a version below 24 |
| Plugins assuming Windows/macOS         | Treat Linux as a supported platform when reading `manifest.json`; shim platform-specific fields                                               |

## Milestones

### M0: Scaffold (this commit)

License, README, contributing guide, scope, architecture, repository layout,
dev tooling, CI.

### M1: Device layer

- `superdeck-device` crate, our own protocol implementation on `hidapi`
  (hidraw) and `udev`: enumerate, hotplug, read key events, write key images,
  brightness, reset, serial/firmware queries for every v1 device
- Per-model protocol tests against recorded USB captures, so device logic is
  testable without hardware
- `superdeck-cli` for manual testing (`list`, `fill`, `image`, `brightness`, `events`)
- Image pipeline per device (size, rotation, mirroring, JPEG/BMP encoding)

### M2: Core model and rendering

- Profile/page/folder/action data model; versioned JSON on-disk format with a
  JSON Schema and migrations, documented in `docs/profile-format.md`
- Key rendering: plugin image + title overlay (`cosmic-text`, bundled Noto
  Sans), SVG, animated GIF
- Core actions (navigation, Multi Action, brightness, sleep)
- Headless run mode that drives a device from a profile file

### M3: Plugin host and SDK

- `sdk/protocol` JSON Schemas as the single source of truth
- `superdeck-plugin-host`: WebSocket server, registration, routing, settings, process supervision
- `@superdeck/sdk` and a "hello world" plugin
- Run a few existing open-source Stream Deck plugins unchanged

### M4: Desktop app

- Tauri shell, tray, single instance, autostart
- Main window at layout parity: canvas, action list, property inspector, drag & drop
- Preferences window, profile management, plugin management

### M5: Bundled plugins

- `plugins/system`: Hotkey, Hotkey Switch, Open, Website, Multimedia, Text
- `plugins/soundboard`: Play Audio, Stop
- Wayland/X11 input backends behind a documented protocol extension

### M6: Linux integration and interoperability

- Smart profiles backends
- `.streamDeckProfile`, `.streamDeckPlugin`, `.streamDeckIconPack` import/export
- Backup/restore, localization infrastructure

### M7: Packaging and 1.0

- Flatpak (Flathub), AppImage, `.deb`, `.rpm`, AUR, Nix flake
- User documentation, plugin compatibility list
- 1.0 release

### After 1.0

Stream Deck +, Pedal, Studio, Neo info bar, Virtual Stream Deck, more bundled
plugins, translations.

## Open questions

None right now. Questions resolved so far are recorded in
[Decisions](#decisions). Raise new ones as issues.
