// SPDX-License-Identifier: MIT OR Apache-2.0

// The plugin's entry point: SuperDeck, or the Stream Deck app, runs the bundle built from it.

import { Plugin } from "@superdeck/sdk";

import { sayHello } from "./hello.ts";

await sayHello(new Plugin()).connect();
