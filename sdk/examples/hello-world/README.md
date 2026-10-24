# Hello World

The smallest useful plugin for SuperDeck, written with
[`@superdeck/sdk`](../../js/). Copy it to start your own.

Its one action, **Say Hello**, shows a greeting on its key, and counts how
often the key was pressed. The greeting is set in the property inspector.
The plugin runs on SuperDeck and on the official Stream Deck app 7.1 or newer.

| Path                                       | What                                                |
| ------------------------------------------ | --------------------------------------------------- |
| `src/hello.ts`                             | The action                                          |
| `src/plugin.ts`                            | The entry point, which connects to the host         |
| `test/`                                    | Tests against the SDK's mock host, no device needed |
| `com.example.hello.sdPlugin/`              | The plugin as the app installs it                   |
| `com.example.hello.sdPlugin/manifest.json` | Name, actions, and what to run                      |
| `com.example.hello.sdPlugin/ui/say.html`   | The property inspector                              |
| `com.example.hello.sdPlugin/bin/`          | The bundle `pnpm build` writes (git-ignored)        |

## Build, test, and run

```sh
pnpm build    # bundles src/ and the SDK into com.example.hello.sdPlugin/bin/plugin.js
pnpm test
```

To try it on a Stream Deck, run a profile with a **Say Hello** key, adding
this directory to the plugin directories:

```sh
superdeck-cli run --plugin-dir sdk/examples/hello-world <profile>
```

To install it, copy `com.example.hello.sdPlugin/` to
`~/.local/share/superdeck/plugins/`. The bundle needs nothing else.

## Making it yours

Change `com.example.hello` to a reverse-DNS ID you own, in the directory name,
`manifest.json`, and `src/hello.ts`, then rename the action.
