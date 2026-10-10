// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * `@superdeck/sdk`: write Stream Deck plugins that run on SuperDeck and the
 * official app alike. See `sdk/protocol/README.md` for the protocol.
 */

export {
  type ActionHandlers,
  type ConnectOptions,
  Key,
  type LaunchArguments,
  Plugin,
  parseArguments,
} from "./plugin.ts";
export type * from "./protocol.ts";
export { type Transport, webSocketTransport } from "./transport.ts";
