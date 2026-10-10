# sdk

Everything plugin authors need. Unlike the rest of the repository, all code
in this directory is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option, so plugins built on it can use any license.

Planned contents (milestone M3):

| Path        | What                                                                                                |
| ----------- | --------------------------------------------------------------------------------------------------- |
| `protocol/` | JSON Schemas for `manifest.json` and every WebSocket message: the contract between host and plugins |
| `js/`       | `@superdeck/sdk`: TypeScript plugin library and a mock host for tests                               |

The protocol is compatible with the Stream Deck SDK, so plugins written with
`@superdeck/sdk` stay close to plugins for the official software, and existing
plugins run on SuperDeck. Any SuperDeck-specific extensions are namespaced and
documented here.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this directory shall be dual licensed as above,
without any additional terms or conditions.
