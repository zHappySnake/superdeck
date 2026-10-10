# crates

Rust libraries that make up the SuperDeck core. Every directory here containing
a `Cargo.toml` is picked up as a workspace member automatically.

| Crate (planned)         | Milestone | Purpose                                             |
| ----------------------- | --------- | --------------------------------------------------- |
| `superdeck-device`      | M1        | USB HID drivers for each Stream Deck model, hotplug |
| `superdeck-cli`         | M1        | Command-line tool for testing devices by hand       |
| `superdeck-render`      | M2        | Key image composition: images, SVG, GIF, titles     |
| `superdeck-core`        | M2        | Profiles, pages, folders, actions, persistence      |
| `superdeck-plugin-host` | M3        | Stream Deck SDK WebSocket host, plugin processes    |

New crates should inherit shared settings from the workspace:

```toml
[package]
name = "superdeck-example"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[lints]
workspace = true
```

See [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md).
