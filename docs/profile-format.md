# Profile format

> [!NOTE]
> **Status: version 1, implemented in `superdeck-core`.** Until the first
> release, version 1 can still change without a migration.

This is how SuperDeck stores profiles, pages, folders, and the actions on
keys. It's our own format: the official `.streamDeckProfile` files are only
read and written by import/export converters (milestone M6), so this format
can follow SuperDeck's needs.

Goals:

- **Readable and diffable.** Plain, pretty-printed JSON that people can back
  up, version-control, and fix by hand.
- **Safe to evolve.** Every file has a format version. Newer SuperDeck
  versions migrate older files; older versions refuse newer files instead of
  damaging them.
- **Portable between devices.** A profile is made for a key layout (5 × 3,
  8 × 4, …), not one device, so it moves to another device with the same
  layout, and is easy to adapt to a different one.
- **Plugin data is opaque.** Settings a plugin stores are kept exactly as
  given, whatever their shape.

## Files on disk

Everything lives in `$XDG_CONFIG_HOME/superdeck/` (usually `~/.config/superdeck/`):

```
superdeck/
├── devices.json                 which profiles each device uses
└── profiles/
    └── 7f9c2e1a-…/              one directory per profile, named by its ID
        ├── profile.json         pages, keys, actions
        └── images/              custom key images, named by content hash
            └── 3a7bd3e2….png
```

A profile is a directory, so that custom images stay next to the profile
without bloating the JSON. Exporting a profile (in SuperDeck's own format)
means zipping that directory.

Other state is outside this document: app preferences
(`settings.json`), plugin global settings (`plugins/<plugin-uuid>/`), and
logs and caches in `$XDG_STATE_HOME` and `$XDG_CACHE_HOME` (see
[ARCHITECTURE.md](ARCHITECTURE.md#files-on-disk)).

## Conventions

- **Encoding.** UTF-8 JSON, written pretty-printed with two-space
  indentation and a trailing newline. Object keys are written in the order
  this document lists them, so files diff cleanly.
- **Names.** Field names are `camelCase`.
- **IDs.** Profiles, pages, and action instances have IDs: random (v4)
  UUIDs in lowercase, such as `"5c0a7e4e-9a42-4e0f-b3c5-0d6c8e2f1b7a"`. An
  ID never changes once created.
- **Optional fields.** A field that's absent means "use the default". Writers
  leave out fields that hold their default, so files stay short. `null` is
  not used.
- **Unknown fields.** A reader rejects fields it doesn't know, except inside
  plugin `settings`. New fields come with a new format version, so an
  unknown field means a damaged file or a newer version.
- **Colors** are `"#rrggbb"`, written in lowercase; uppercase is accepted
  when reading.

## `profile.json`

```json
{
  "formatVersion": 1,
  "id": "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b",
  "name": "Streaming",
  "layout": { "columns": 5, "rows": 3 },
  "applications": ["com.obsproject.Studio"],
  "pages": [
    {
      "id": "0b1c2d3e-4f50-4617-a8b9-cadbecfd0e1f",
      "keys": {
        "0,0": {
          "id": "5c0a7e4e-9a42-4e0f-b3c5-0d6c8e2f1b7a",
          "action": "io.github.zhappysnake.superdeck.system.hotkey",
          "plugin": "io.github.zhappysnake.superdeck.system",
          "settings": { "keys": ["Ctrl", "Shift", "M"] },
          "appearance": [{ "title": "Mute", "titleAlignment": "bottom" }]
        }
      }
    }
  ]
}
```

| Field           | Type                             | Default  | Meaning                                                                                      |
| --------------- | -------------------------------- | -------- | -------------------------------------------------------------------------------------------- |
| `formatVersion` | integer                          | required | Format version of this file; see [Versions and migrations](#versions-and-migrations)         |
| `id`            | UUID                             | required | Profile ID; also the directory name                                                          |
| `name`          | string                           | required | Name shown in the profile list                                                               |
| `layout`        | [Layout](#layout)                | required | Key grid the profile is made for                                                             |
| `applications`  | array of strings                 | `[]`     | Application IDs that switch to this profile when focused ([smart profiles](#smart-profiles)) |
| `source`        | [Source](#profiles-from-plugins) | none     | Plugin profile this one was installed from                                                   |
| `pages`         | array of [Page](#pages)          | required | At least one page; shown in this order                                                       |

### Layout

```json
{ "columns": 5, "rows": 3 }
```

The key grid, not the device model, because models share grids:

| Grid  | Devices                                |
| ----- | -------------------------------------- |
| 3 × 2 | Mini, Mini MK.2, Module 6              |
| 4 × 2 | Neo                                    |
| 5 × 3 | Original, Original v2, MK.2, Module 15 |
| 8 × 4 | XL, XL v2, Module 32                   |

Key image size differs between devices with the same grid; rendering scales
to each device, so the profile doesn't care. Stream Deck + dials and the Neo
info bar will add optional fields here (for example `"dials": 4`) in a later
format version.

### Pages

```json
{ "id": "0b1c2d3e-…", "keys": { "0,0": { … }, "4,2": { … } } }
```

| Field  | Type                                            | Default  | Meaning                  |
| ------ | ----------------------------------------------- | -------- | ------------------------ |
| `id`   | UUID                                            | required | Page ID                  |
| `keys` | object of [action instances](#action-instances) | `{}`     | Keys that have an action |

Keys are addressed as `"column,row"`, counted from 0 at the top left, so
`"4,2"` is the bottom-right key of a 5 × 3 layout. Positions outside the
layout are an error. Empty keys are left out.

Addressing by position rather than key index means a profile adapted to a
bigger layout keeps its arrangement (the top-left 5 × 3 of an XL page looks
like the original page), and an object rules out two actions on one key.

## Action instances

An action instance is one action placed on a key, with its settings and
look. The same object describes plugin actions and the
[core actions](#core-actions).

| Field        | Type                               | Default  | Meaning                                            |
| ------------ | ---------------------------------- | -------- | -------------------------------------------------- |
| `id`         | UUID                               | required | Instance ID; plugins see it as the event `context` |
| `action`     | string                             | required | Action UUID, as in the plugin's `manifest.json`    |
| `plugin`     | string                             | required | Plugin UUID                                        |
| `settings`   | object                             | `{}`     | Per-action settings, as the plugin last saved them |
| `state`      | integer                            | `0`      | Current state of a multi-state action (0 or 1)     |
| `appearance` | array of [Appearance](#appearance) | `[]`     | User customizations, one per state, in state order |

- `plugin` is stored, not looked up, so a key whose plugin isn't installed
  still knows what it's waiting for. Such keys keep all their data and show
  a placeholder until the plugin is installed.
- `settings` belong to the plugin (`setSettings`/`didReceiveSettings`) and
  are stored as received, with no checks beyond being a JSON object.
- `state` is stored because the official app keeps a toggle's state across
  restarts.
- `id` doubles as the SDK `context`, so a plugin sees the same context for
  the same key across restarts. Copying or duplicating a key gives the copy
  a new `id`.

### Appearance

What the user set in the property inspector for one state: title, title
style, and image. Every field is optional; an absent field means the user
hasn't changed it, so the plugin's value (from `setTitle`/`setImage`) or the
manifest default applies.

```json
{
  "title": "Mute",
  "showTitle": true,
  "titleAlignment": "bottom",
  "fontFamily": "Noto Sans",
  "fontSize": 12,
  "fontStyle": "bold",
  "fontUnderline": false,
  "titleColor": "#ffffff",
  "image": "images/3a7bd3e2c4….png"
}
```

| Field            | Type                                              | Meaning                                                                |
| ---------------- | ------------------------------------------------- | ---------------------------------------------------------------------- |
| `title`          | string                                            | Title text; `\n` separates lines. `""` is an intentionally empty title |
| `showTitle`      | boolean                                           | Whether the title is drawn                                             |
| `titleAlignment` | `"top"`, `"middle"`, `"bottom"`                   | Vertical position                                                      |
| `fontFamily`     | string                                            | Font family name; the bundled Noto Sans if not installed               |
| `fontSize`       | number                                            | Pixels on a 72-pixel key, scaled for other key sizes; 1 to 144         |
| `fontStyle`      | `"regular"`, `"bold"`, `"italic"`, `"boldItalic"` | Font style                                                             |
| `fontUnderline`  | boolean                                           | Underlined                                                             |
| `titleColor`     | color                                             | Text color                                                             |
| `image`          | string                                            | Custom image, as a path inside the profile directory                   |

The names follow the SDK's title parameters (`titleParametersDidChange`), so
mapping to and from plugins is direct. `fontStyle` uses its own lowercase
values; the SDK's `"Bold Italic"` and friends are converted at the plugin
boundary.

Values set by plugins at run time (`setTitle`, `setImage`) are not stored.
Plugins set them again when the key appears (`willAppear`), which is also
what the official app expects of them.

What a key shows is decided field by field: the user's `appearance` value
if there is one, else the plugin's run-time value, else the manifest
default. A title or image the user set by hand always stays, even if a
plugin keeps updating its own; clearing the field in the property inspector
hands it back to the plugin. A plugin's `setTitle` or `setImage` without a
value clears only the plugin's value.

### Images

A custom image is stored once, in the profile's `images/` directory, as the
original file bytes (PNG, JPEG, GIF, or SVG), named by the SHA-256 of its
contents plus the format's extension. Keeping the original rather than a
rendered bitmap means images stay sharp on any key size and animated GIFs
keep their frames. Naming by content means the same image on many keys is
stored once, and an image file never changes once written.

`image` holds the path relative to the profile directory
(`"images/<hash>.<ext>"`); any other path is an error, so a profile can't
point outside its directory. Image files no key refers to are deleted when
the profile is saved. If an image file is missing, the key shows without it;
the profile still loads.

## Core actions

The built-in actions use the reserved plugin UUID
`io.github.zhappysnake.superdeck` and keep their data in `settings`, like any
plugin action. Unlike plugin settings, these settings are part of this
format: the schema checks them and migrations update them.

| Action (after the `io.github.zhappysnake.superdeck.` prefix) | `settings`                                                                              |
| ------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `folder`                                                     | `{ "keys": { … } }`: the folder's own key grid, as in a [page](#pages)                  |
| `switch-profile`                                             | `{ "profile": "<profile id>", "page": <index> }`; `page` defaults to 0                  |
| `next-page`, `previous-page`                                 | `{}`                                                                                    |
| `go-to-page`                                                 | `{ "page": <index> }`                                                                   |
| `page-indicator`                                             | `{}`                                                                                    |
| `multi-action`                                               | `{ "steps": [ <action instance>, … ] }`                                                 |
| `multi-action-switch`                                        | `{ "steps": [[ <action instance>, … ], [ <action instance>, … ]] }`, one list per state |
| `delay`                                                      | `{ "milliseconds": <integer> }`; only valid as a Multi Action step                      |
| `brightness`                                                 | `{ "mode": "set" \| "increase" \| "decrease", "value": <0–100> }`                       |
| `sleep`                                                      | `{}`                                                                                    |

- **Folders** nest to any depth: a folder's `keys` can hold another folder.
  The top-left key of an open folder is the back key, drawn by SuperDeck, so
  `"0,0"` must stay empty in a folder's `keys`.
- **Multi Action steps** are action instances, with their own `id` and
  `settings`, run in order. They have no appearance, and can't be folders,
  Multi Actions, or page navigation.
- **Page numbers** are indexes into the profile's `pages`. When pages are
  reordered or deleted, SuperDeck updates `go-to-page` and `switch-profile`
  references to keep pointing at the same page; a reference to a deleted page
  goes back to 0.
- **Switch Profile** to a profile that no longer exists does nothing.

## `devices.json`

Which profiles each device uses, and its device settings.

```json
{
  "formatVersion": 1,
  "devices": {
    "AL12K1A01234": {
      "name": "Desk",
      "profiles": [
        "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b",
        "c3d4e5f6-0718-4293-a4b5-c6d7e8f90a1b"
      ],
      "defaultProfile": "7f9c2e1a-3b4d-4e5f-8a6b-7c8d9e0f1a2b",
      "brightness": 60,
      "sleepAfterMinutes": 30
    }
  }
}
```

Devices are keyed by serial number, which stays the same whichever USB port
they're plugged into.

| Field               | Type           | Default             | Meaning                                                    |
| ------------------- | -------------- | ------------------- | ---------------------------------------------------------- |
| `name`              | string         | the model name      | Name shown in the device selector                          |
| `profiles`          | array of UUIDs | required            | The device's profiles, in the order the profile list shows |
| `defaultProfile`    | UUID           | the first profile   | Profile shown at start, and when no smart profile matches  |
| `brightness`        | integer        | `60`                | Backlight, 0 to 100                                        |
| `sleepAfterMinutes` | integer        | none (never sleeps) | Turn the screen off after this many idle minutes           |

A profile belongs to one device. Its layout must match the device's;
assigning a profile to a device with another layout makes a copy for the
new layout, leaving the original unchanged. Keys that don't fit the new grid
are left out of the copy. A profile directory that no device lists is kept and offered for
reassignment, so unplugging a device never loses its profiles.

A device seen for the first time gets a new, empty profile named "Default".

Which profile and page is currently showing is not stored here: it is run
state, kept in `$XDG_STATE_HOME/superdeck/state.json` so that switching
pages doesn't rewrite configuration.

## Profiles from plugins

A plugin can ship profiles in its package and switch a device to them
(`switchToProfile`). They're installed as ordinary profiles that the user
can edit, rename, and delete like any other, whatever the plugin's manifest
says about read-only profiles.

```json
"source": { "plugin": "com.example.streaming", "profile": "Streaming Layout" }
```

| Field     | Type   | Meaning                                            |
| --------- | ------ | -------------------------------------------------- |
| `plugin`  | string | UUID of the plugin that shipped the profile        |
| `profile` | string | The profile's name in the plugin's `manifest.json` |

- When a plugin is installed, or a device is first seen, each profile the
  plugin ships for that device type is installed once for each matching
  device, with `source` set.
- `switchToProfile` from that plugin finds the profile by `source`, so it
  still works after the user renames or edits it. If the user deleted it,
  the request does nothing; deleted profiles aren't installed again.
- Plugin updates never overwrite an installed copy, and removing the plugin
  leaves its profiles in place.

## Smart profiles

A profile's `applications` lists the application IDs that should switch the
device to it when they get focus: the Wayland `app_id` (often the desktop
file name, such as `com.obsproject.Studio`) or, on X11, the `WM_CLASS`
class. The profile editor offers the IDs of running applications, so users
rarely type them. When the focused
application is in no profile's list, the device returns to its
`defaultProfile`. If two of a device's profiles list the same application,
the first in the device's `profiles` order wins.

## Versions and migrations

Every file starts with `formatVersion`, an integer that increases by one
for every incompatible change. A file is read like this:

1. **Same version:** read it.
2. **Older:** run the migrations in order (1 → 2, 2 → 3, …), each a pure
   function from one version's JSON to the next. Before the migrated file
   is first written back, the original is kept as `profile.json.v1.bak` (with
   its old version number). Migrations are tested against sample files of
   every old version, kept in the repository.
3. **Newer:** don't load it, and never write over it; SuperDeck says that a
   newer version made it. Downgrading never destroys data.

The format version is separate from the app version; most releases won't
change it.

## Writing files

- A file is written to a temporary file in the same directory, flushed to
  disk, and then renamed over the old one, so a crash or power loss leaves
  either the old or the new file, never half of one.
- Image files are written before the `profile.json` that refers to them, and
  orphaned images are deleted after it.
- Changes are saved shortly after they happen (after a short delay that
  groups bursts of edits), not only on exit.
- If a file can't be read (invalid JSON, schema errors), it's moved aside as
  `profile.json.broken-<timestamp>` and the user is told; SuperDeck never
  overwrites a file it couldn't read.

## Validation

`superdeck-core` ships a JSON Schema for each file and version, in
[`crates/superdeck-core/schema/`](../crates/superdeck-core/schema/)
(`profile.v1.schema.json`, `devices.v1.schema.json`), with sample files in
[`crates/superdeck-core/tests/data/`](../crates/superdeck-core/tests/data/).
Tests check that the samples and everything SuperDeck writes match the
schemas. The schema covers structure, value ranges, and core action
settings; the rules it can't express are checked in code:

- key positions are inside the layout, and `"0,0"` is empty in folders
- IDs are unique within a profile (pages and action instances, including
  nested ones)
- a profile's `id` matches its directory name
- `defaultProfile` is one of the device's `profiles`, and no profile belongs
  to two devices
- Multi Action steps are allowed actions
