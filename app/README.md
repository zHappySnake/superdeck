# app

The SuperDeck desktop application, built with Tauri v2. It doesn't exist yet;
it arrives in milestone M4 of the [roadmap](../docs/ROADMAP.md).

Planned layout:

```
app/
├── package.json        Svelte 5 + Vite + TypeScript frontend package
├── src/                UI that mirrors the official app's layout
└── src-tauri/          Rust shell (crate `superdeck-app`): window, tray, core wiring
```

When `src-tauri/` is added, list it in the root `Cargo.toml` workspace
`members`.
