# SuperDeck

**A native Linux app for Elgato Stream Deck hardware.** It aims to look and
behave like the official Windows Stream Deck software, minus the marketplace
and online features, and to run existing Stream Deck SDK plugins.

> [!WARNING]
> **Status: pre-alpha.** The device layer (milestone M1) and key rendering
> are in place, with a command-line tool for testing devices; there's no app
> yet. See the [roadmap](docs/ROADMAP.md) for what comes next.

## Goals

- **Familiar UI.** Same layout, workflow, and interactions as the official
  Windows app (device canvas, action list, property inspector, profiles,
  pages, folders, multi actions), so anyone who knows Stream Deck feels at
  home. All artwork is original; see [Trademarks](#trademarks).
- **Works with the official devices** over USB HID, with no Elgato software
  needed.
- **Stream Deck SDK-compatible plugin API.** Plugins are separate processes
  talking to SuperDeck over the same WebSocket protocol and `manifest.json`
  format as the official SDK. Many existing open-source `.sdPlugin` plugins
  should work without changes.
- **Batteries included.** The standard actions (hotkeys, open app/file,
  website, multimedia, text, soundboard, …) ship as bundled plugins. They live
  in this repository but use only the public plugin API, exactly like
  third-party plugins.
- **Offline and private.** No account, no telemetry, no network access unless
  a plugin you installed does it.
- **A good Linux citizen.** Works on Wayland and X11, follows XDG
  directories, uses D-Bus/MPRIS/PipeWire, and is packaged for Flatpak,
  AppImage, `.deb`, `.rpm`, AUR and Nix.

### Non-goals

Marketplace, Elgato account and cloud sync, auto-updater (your package
manager does that), telemetry, Stream Deck Mobile pairing, firmware updates,
and Windows/macOS ports. Plugins that ship only Windows or macOS native
binaries won't run. See [the roadmap](docs/ROADMAP.md#non-goals) for the
reasoning.

## Device support

| Device                                | Status  |
| ------------------------------------- | ------- |
| Stream Deck (original, v2), MK.2      | v1.0    |
| Stream Deck Mini, Mini MK.2           | v1.0    |
| Stream Deck XL (v1, v2)               | v1.0    |
| Stream Deck Neo (keys)                | v1.0    |
| Stream Deck Module (6/15/32)          | v1.0    |
| Stream Deck Neo info bar & touch keys | planned |
| Stream Deck + (dials, touch strip)    | planned |
| Stream Deck Pedal                     | planned |
| Stream Deck Studio                    | planned |
| Virtual Stream Deck (on-screen)       | planned |

## Architecture at a glance

```
┌─────────────────────── superdeck (single process) ───────────────────────┐
│  UI: Svelte 5 + TypeScript in a Tauri v2 webview (WebKitGTK)             │
│        ⇅ Tauri IPC                                                       │
│  Core (Rust): profiles · pages · folders · multi actions · rendering     │
│        ├── Device layer (hidraw) ──────── USB ──── Stream Deck devices   │
│        └── Plugin host (WebSocket, Stream Deck SDK protocol)             │
└───────────────────────────────┬──────────────────────────────────────────┘
                                │ ws://127.0.0.1:<port>
        ┌───────────────────────┼─────────────────────────┐
  bundled plugins       third-party plugins     property inspectors
  (Node.js, this repo)  (~/.local/share/…)      (HTML, inside the UI)
```

Details are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Repository layout

```
app/         Tauri desktop app (Rust shell in app/src-tauri, Svelte UI in app/src)
crates/      Rust libraries: device I/O, core model, rendering, plugin host
sdk/         Plugin SDK and protocol schemas (MIT OR Apache-2.0)
plugins/     Bundled plugins: independent packages using only the SDK
packaging/   udev rules, Flatpak, AppImage, deb/rpm, AUR, Nix recipes
docs/        Roadmap, architecture, protocol notes
```

## Building

There's no app to run yet. What exists is `superdeck-cli`, a tool for driving
a Stream Deck by hand:

```sh
cargo run -p superdeck-cli -- list            # connected devices
cargo run -p superdeck-cli -- info            # model, serial, firmware
cargo run -p superdeck-cli -- test-pattern    # check image orientation
cargo run -p superdeck-cli -- events          # print key presses
cargo run -p superdeck-cli -- render -i icon.svg -t Mute --align bottom
                                              # draw a key as the app will
cargo run -p superdeck-cli -- --help          # everything else
```

If you own a Stream Deck, checking it this way helps a lot; see
[docs/device-protocol.md](docs/device-protocol.md#verification-status). To set
up a development environment and run the repository checks, see
[CONTRIBUTING.md](CONTRIBUTING.md).

### Device permissions

Linux only lets root open HID devices unless a udev rule grants access. The
packages will install [`packaging/udev/70-superdeck.rules`](packaging/udev/70-superdeck.rules)
for you. For development, install it by hand:

```sh
sudo cp packaging/udev/70-superdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first,
especially the **clean-room policy**: do not contribute anything copied,
decompiled, or extracted from Elgato's software.

## License

SuperDeck is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. See [LICENSE](LICENSE).

Exception: the plugin SDK and protocol schemas in [`sdk/`](sdk/) are licensed
under either the [MIT](sdk/LICENSE-MIT) or [Apache-2.0](sdk/LICENSE-APACHE)
license, at your option, so plugins can use any license.

The bundled Noto Sans fonts in
[`crates/superdeck-render/fonts/`](crates/superdeck-render/fonts/) are under
the [SIL Open Font License 1.1](crates/superdeck-render/fonts/OFL.txt).

## Trademarks

SuperDeck is an independent project. It is not affiliated with, endorsed by,
or sponsored by Elgato or Corsair. "Elgato" and "Stream Deck" are trademarks
of Corsair Memory, Inc. and are used here only to name the hardware SuperDeck
works with. SuperDeck does not include Elgato's code, icons, artwork, or other
assets.

## Related projects

- [OpenDeck](https://github.com/nekename/OpenDeck): Rust/Tauri app with
  Stream Deck plugin support
- [StreamController](https://github.com/StreamController/StreamController):
  Python/GTK Stream Deck app for Linux
- [python-elgato-streamdeck](https://github.com/abcminiuser/python-elgato-streamdeck):
  Python library, a valuable reference for the HID protocol
- [elgato-streamdeck](https://crates.io/crates/elgato-streamdeck): Rust
  library for the same devices, another protocol reference
