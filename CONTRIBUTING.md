# Contributing to SuperDeck

Thanks for your interest! SuperDeck is at an early stage, so the most useful
contributions right now are design discussion, protocol research, and hardware
testing. Code contributions follow the [roadmap](docs/ROADMAP.md) milestones.

- [Ground rules](#ground-rules)
- [Development setup](#development-setup)
- [Repository layout](#repository-layout)
- [Day-to-day workflow](#day-to-day-workflow)
- [Code style](#code-style)
- [Plugins](#plugins)
- [Commits and pull requests](#commits-and-pull-requests)
- [Licensing of contributions](#licensing-of-contributions)
- [Reporting hardware and bugs](#reporting-hardware-and-bugs)

## Ground rules

### Clean-room policy

SuperDeck reproduces the **look and behavior** of the official Stream Deck
software, never its **code or assets**. To keep the project legally safe:

- **Do not** contribute code, icons, images, sounds, fonts, translations, or
  other files copied or extracted from Elgato's software, installers, or
  plugins, and **do not** decompile or disassemble their binaries.
- **Do** use public documentation (the Stream Deck SDK docs and the MIT
  licensed `@elgato/*` npm packages, keeping their license notices), USB
  traffic captures of your own devices, screenshots for layout reference, and
  existing open-source projects under compatible licenses (with attribution).
- Recreate UI artwork from scratch, or use icon sets under a GPL-compatible
  license (for example MIT, Apache-2.0, or OFL for fonts). Record the source
  and license of every third-party asset next to it.

If you're unsure whether something is acceptable, ask in an issue before you
open a pull request.

### Scope

The marketplace, Elgato accounts, cloud features, and telemetry are
**permanent non-goals** (see the [roadmap](docs/ROADMAP.md#non-goals)).
Pull requests adding them will be closed.

## Development setup

### Toolchains

| Tool            | Version                         | How to get it                          |
| --------------- | ------------------------------- | -------------------------------------- |
| Rust            | pinned in `rust-toolchain.toml` | [rustup](https://rustup.rs)            |
| Node.js         | 24 LTS (`.nvmrc`)               | distro package, nvm, fnm, or mise      |
| pnpm            | pinned in `package.json`        | `corepack enable`                      |
| just (optional) | any recent                      | distro package or `cargo install just` |

rustup reads `rust-toolchain.toml` and installs the right compiler, `rustfmt`
and `clippy` automatically.

### System libraries

The Rust crates need a C compiler, `pkg-config`, and the libudev headers
(`hidapi` compiles its C sources). The rest of the list below is for the
Tauri app, which doesn't exist yet; installing it now saves a step later.

**Debian / Ubuntu**

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libudev-dev libssl-dev
```

**Fedora**

```sh
sudo dnf install gcc pkgconf-pkg-config webkit2gtk4.1-devel gtk3-devel \
  libappindicator-gtk3-devel librsvg2-devel systemd-devel openssl-devel
```

**Arch**

```sh
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 libappindicator-gtk3 \
  librsvg systemd-libs openssl
```

### Device access

Install the udev rule so your user can open Stream Deck devices without root:

```sh
sudo cp packaging/udev/70-superdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then unplug and replug the device. Only one program can drive a device at a
time, so close other Stream Deck software while testing.

### First run

```sh
corepack enable
pnpm install
just check        # or: pnpm format:check && pnpm lint && pnpm typecheck && pnpm test
```

## Repository layout

| Path            | What                                                | License           |
| --------------- | --------------------------------------------------- | ----------------- |
| `app/`          | Tauri app: `src-tauri/` (Rust), `src/` (Svelte UI)  | GPL-3.0-or-later  |
| `crates/`       | Rust libraries (device, core, render, plugin host)  | GPL-3.0-or-later  |
| `sdk/protocol/` | JSON Schemas for `manifest.json` and all messages   | MIT OR Apache-2.0 |
| `sdk/js/`       | TypeScript plugin SDK (`@superdeck/sdk`)            | MIT OR Apache-2.0 |
| `plugins/`      | Bundled plugins, one independent package per folder | GPL-3.0-or-later  |
| `packaging/`    | udev rules and distribution recipes                 | GPL-3.0-or-later  |
| `docs/`         | Design docs                                         | GPL-3.0-or-later  |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit.

## Day-to-day workflow

`just` is the umbrella task runner. Every recipe is a thin wrapper, so you can
run the underlying `cargo` / `pnpm` commands yourself.

| Command      | What it does                                            |
| ------------ | ------------------------------------------------------- |
| `just`       | list recipes                                            |
| `just setup` | install JS dependencies                                 |
| `just fmt`   | format Rust and JS/TS/Svelte/Markdown/YAML              |
| `just lint`  | `cargo clippy` (warnings are errors) and ESLint         |
| `just test`  | Rust and JS test suites                                 |
| `just check` | everything CI runs: format check, lint, typecheck, test |
| `just deny`  | dependency license/advisory audit (needs `cargo-deny`)  |

CI (`.github/workflows/ci.yml`) runs the same checks on every push and pull
request. Run `just check` before you push.

## Code style

- **Rust:** `rustfmt` (config in `rustfmt.toml`) and Clippy with the
  workspace lints in `Cargo.toml` (`clippy::pedantic` as warnings, which CI
  treats as errors). `unsafe` is denied by default; if a module really needs
  it, allow it locally with a `// SAFETY:` comment on every block. Use
  `thiserror` for library errors and `anyhow` only in binaries.
- **TypeScript / Svelte:** Prettier (`.prettierrc.json`) and ESLint
  (`eslint.config.js`). `strict` TypeScript everywhere; avoid `any`.
- **Everything else:** follow `.editorconfig`. Markdown and YAML go through
  Prettier as well.
- **SPDX headers:** start every new source file with an SPDX line matching the
  license of the directory it's in:

  ```rust
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```

  ```ts
  // SPDX-License-Identifier: MIT OR Apache-2.0
  ```

## Plugins

Bundled plugins live in this repository and ship with the app, but they must
stay **independent of the program's code**. Treat each one as if it were a
third-party plugin in a separate repository:

1. **Own package.** Each plugin is a self-contained folder under `plugins/`
   with its own `package.json`, build, tests, and `*.sdPlugin` output.
2. **Public API only.** A plugin may depend on `@superdeck/sdk` and public npm
   packages. It must never import from `app/`, `crates/`, or another plugin.
   ESLint enforces this.
3. **Protocol only.** Plugins talk to the host over the documented WebSocket
   protocol, never through private hooks. If a plugin needs a capability the
   protocol lacks, extend the protocol (documented, schema'd, namespaced) so
   third-party plugins get it too.
4. **Standard output.** A plugin's build produces a normal `.sdPlugin`
   directory. Packaging copies the built directories into the app bundle;
   nothing in the app is compiled against plugin sources.

The few structural actions that contain other actions (Multi Action, folders,
page navigation) are part of the core, as in the official app. Everything else
belongs in a plugin.

## Commits and pull requests

- Branch from `main`; keep pull requests focused on one change.
- Sign off every commit (`git commit -s`); see
  [Developer Certificate of Origin](#developer-certificate-of-origin).
- Use [Conventional Commits](https://www.conventionalcommits.org/) for
  messages, scoped by area where it helps:
  `feat(device): support Stream Deck Neo`,
  `fix(plugin-host): reject messages for unknown contexts`,
  `docs: describe profile file format`.
- Include tests for new behavior. For device work, say which hardware you
  tested on in the PR description.
- Changes to the protocol or plugin API need a schema update in
  `sdk/protocol/` and a note in the docs.
- Be kind; assume good faith.

## Licensing of contributions

By submitting a contribution you agree that it's licensed under the license
of the directory it goes into ("inbound = outbound"): GPL-3.0-or-later for
most of the repository, MIT OR Apache-2.0 for `sdk/`. No CLA is required. You
keep the copyright to your work. Only contribute work you have the right to
license this way.

### Developer Certificate of Origin

Every commit must carry a `Signed-off-by:` line with your real name and email,
certifying the [Developer Certificate of Origin 1.1](https://developercertificate.org/):
that you wrote the change or otherwise have the right to submit it under the
project's licenses. Git adds the line for you:

```sh
git commit -s -m "fix(device): handle short reads"
```

To sign off commits you already made on your branch, run
`git rebase --signoff main` and force-push the branch. A CI check fails pull
requests that contain commits without a sign-off.

When adding a Rust or npm dependency, check that its license is
GPL-3.0-compatible (`just deny` checks Rust crates against `deny.toml`). For
`sdk/`, dependencies must also be permissively licensed.

## Reporting hardware and bugs

Use the issue templates. For device problems, include the model (`lsusb`
output helps), firmware version if known, distribution, desktop environment,
and whether you're on Wayland or X11.
