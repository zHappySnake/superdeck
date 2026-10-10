// SPDX-License-Identifier: MIT OR Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";

import { Plugin, parseArguments } from "../src/index.ts";
import { MockHost } from "../src/testing.ts";

test("arguments are read and checked", () => {
  const info = { plugin: { uuid: "a.b", version: "1" } };
  const launch = parseArguments([
    "-port",
    "123",
    "-pluginUUID",
    "t",
    "-registerEvent",
    "registerPlugin",
    "-info",
    JSON.stringify(info),
  ]);
  assert.equal(launch.port, 123);
  assert.equal(launch.pluginUUID, "t");
  assert.throws(() => parseArguments(["-port", "1"]), /missing/);
});

test("actions get their events and answer through keys", async () => {
  const host = new MockHost();
  const plugin = new Plugin();
  plugin.action("com.example.plugin.say", {
    async keyDown(_event, key) {
      const settings = await key.getSettings();
      key.setTitle(`n=${String(settings["n"])}`);
    },
  });
  await plugin.connect(host.options());
  assert.ok(host.registered);
  host.keyDown({ action: "com.example.plugin.say", settings: { n: 2 } });
  assert.equal((await host.nextCommand()).event, "getSettings");
  assert.deepEqual(await host.nextCommand(), {
    event: "setTitle",
    context: "context-1",
    payload: { title: "n=2" },
  });
  plugin.setGlobalSettings({ token: "x" });
  await host.nextCommand();
  assert.deepEqual(await plugin.getGlobalSettings(), { token: "x" });
  assert.equal(plugin.devices.size, 1);
});
