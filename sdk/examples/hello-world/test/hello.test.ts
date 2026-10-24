// SPDX-License-Identifier: MIT OR Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";

import { Plugin } from "@superdeck/sdk";
import { MockHost } from "@superdeck/sdk/testing";

import { SAY_HELLO, sayHello, title } from "../src/hello.ts";

async function connected(): Promise<MockHost> {
  const host = new MockHost({ plugin: { uuid: "com.example.hello", version: "1.0.0.0" } });
  await sayHello(new Plugin()).connect(host.options());
  return host;
}

test("the title is the greeting and the count", () => {
  assert.equal(title({}), "Hello");
  assert.equal(title({ greeting: "  " }), "Hello");
  assert.equal(title({ greeting: "Hi", count: 3 }), "Hi\n×3");
});

test("a key says hello when it appears", async () => {
  const host = await connected();
  host.willAppear({ action: SAY_HELLO });
  assert.deepEqual(await host.nextCommand(), {
    event: "setTitle",
    context: "context-1",
    payload: { title: "Hello" },
  });
});

test("pressing the key counts, and keeps the greeting", async () => {
  const host = await connected();
  host.keyDown({ action: SAY_HELLO, settings: { greeting: "Hi", count: 1 } });
  assert.deepEqual(await host.nextCommand(), {
    event: "setSettings",
    context: "context-1",
    payload: { greeting: "Hi", count: 2 },
  });
  assert.deepEqual(await host.nextCommand(), {
    event: "setTitle",
    context: "context-1",
    payload: { title: "Hi\n×2" },
  });
  assert.equal((await host.nextCommand()).event, "showOk");
  assert.deepEqual(host.settings.get("context-1"), { greeting: "Hi", count: 2 });
});

test("a greeting from the property inspector shows at once", async () => {
  const host = await connected();
  host.inspectorSetSettings({ action: SAY_HELLO }, { greeting: "Hey" });
  assert.deepEqual(await host.nextCommand(), {
    event: "setTitle",
    context: "context-1",
    payload: { title: "Hey" },
  });
});
