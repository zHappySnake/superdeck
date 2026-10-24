// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Checks messages against the protocol's JSON Schemas in `sdk/protocol/schemas/`, so the SDK and
 * its mock host can't drift from the contract the real host follows.
 */

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

import { Ajv2020 } from "ajv/dist/2020.js";

import type { ConnectOptions } from "../src/plugin.ts";
import { parseArguments } from "../src/plugin.ts";
import type { MockHost } from "../src/testing.ts";

const DIRECTORY = new URL("../../protocol/schemas/", import.meta.url);
const BASE = "https://github.com/zhappysnake/superdeck/sdk/protocol/schemas/";

// Strict, but `"type": [...]` unions are valid JSON Schema, used where manifests vary.
const ajv = new Ajv2020({ allErrors: true, strict: true, allowUnionTypes: true });
// The schemas refer to each other by their $id.
for (const name of readdirSync(DIRECTORY)) {
  ajv.addSchema(JSON.parse(readFileSync(new URL(name, DIRECTORY), "utf8")) as object);
}

/** Every schema, by file name. */
export const schemaNames: readonly string[] = readdirSync(DIRECTORY);

/** Why the schema in `sdk/protocol/schemas/<schema>` rejects `value`, or `undefined` if it doesn't. */
export function rejection(schema: string, value: unknown): string | undefined {
  const validate = ajv.getSchema(BASE + schema);
  if (!validate) return `no schema ${schema}`;
  if (validate(value)) return undefined;
  const errors = ajv.errorsText(validate.errors, { separator: "\n" });
  return `${schema} rejects ${JSON.stringify(value, null, 2)}:\n${errors}`;
}

/** Fails unless `value` matches the schema in `sdk/protocol/schemas/<schema>`. */
export function assertValid(schema: string, value: unknown): void {
  const problem = rejection(schema, value);
  if (problem) assert.fail(problem);
}

/**
 * Checks everything that passes between a mock host and the plugin: `-info`, the registration,
 * each command the plugin sends, and each event the host sends. Pass `options` to
 * `plugin.connect()`, and call `assertAllValid()` at the end of the test; failing in the middle
 * would only reach the plugin's error handling.
 */
export function checked(host: MockHost): { options: ConnectOptions; assertAllValid(): void } {
  const { argv = [], transport } = host.options();
  assert.ok(transport);
  const problems: string[] = [];
  const check = (schema: string, text: string) => {
    const problem = rejection(schema, JSON.parse(text));
    if (problem) problems.push(problem);
  };
  check("registration-info.schema.json", JSON.stringify(parseArguments(argv).info));
  let registered = false;
  return {
    options: {
      argv,
      transport: {
        send(text) {
          check(registered ? "plugin-commands.schema.json" : "registration.schema.json", text);
          registered = true;
          transport.send(text);
        },
        onMessage(listener) {
          transport.onMessage((text) => {
            check("plugin-events.schema.json", text);
            listener(text);
          });
        },
        onClose: (listener) => transport.onClose(listener),
        close: () => transport.close(),
      },
    },
    assertAllValid() {
      assert.deepEqual(problems, []);
    },
  };
}
