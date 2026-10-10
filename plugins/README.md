# plugins

Plugins bundled with SuperDeck. They ship with the app but are **independent of
its code**: each folder is a self-contained package that depends only on
[`@superdeck/sdk`](../sdk/) and talks to the host over the public protocol,
exactly like a third-party plugin. See
[CONTRIBUTING.md § Plugins](../CONTRIBUTING.md#plugins) for the rules.

| Plugin (planned) | ID                                           | Actions                                                |
| ---------------- | -------------------------------------------- | ------------------------------------------------------ |
| `system/`        | `io.github.zhappysnake.superdeck.system`     | Hotkey, Hotkey Switch, Open, Website, Multimedia, Text |
| `soundboard/`    | `io.github.zhappysnake.superdeck.soundboard` | Play Audio, Stop                                       |

Planned layout of a plugin:

```
plugins/system/
├── package.json                                   name, build/test scripts, depends on @superdeck/sdk
├── src/                                           plugin source (TypeScript)
├── tests/
└── io.github.zhappysnake.superdeck.system.sdPlugin/
    ├── manifest.json
    ├── imgs/                                      original artwork only
    ├── ui/                                        property inspector HTML
    └── bin/                                       build output (git-ignored)
```
