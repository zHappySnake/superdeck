// SPDX-License-Identifier: MIT OR Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";

import { type Key, Plugin } from "../src/index.ts";
import { MockHost } from "../src/testing.ts";
import { checked, rejection, schemaNames } from "./schemas.ts";

const ACTION = "com.example.plugin.say";

test("every schema compiles", () => {
  for (const name of schemaNames) {
    // Compiling in strict mode throws on unknown keywords and broken references.
    assert.doesNotMatch(rejection(name, null) ?? "", /^no schema/, name);
  }
});

test("every command the SDK sends matches the schemas", async () => {
  const host = new MockHost();
  const { options, assertAllValid } = checked(host);
  const plugin = new Plugin();
  let key: Key | undefined;
  plugin.action(ACTION, {
    willAppear(_event, k) {
      key = k;
    },
  });
  await plugin.connect(options);
  host.willAppear({ action: ACTION, settings: { n: 1 }, resources: { sound: "/a.wav" } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.ok(key);

  key.setTitle("Hi");
  key.setTitle("Hi", 1);
  key.setTitle();
  key.setImage("data:image/png;base64,iVBORw0KGgo=");
  key.setImage("imgs/key", 0);
  key.setImage();
  key.setState(1);
  key.showAlert();
  key.showOk();
  key.setSettings({ n: 2, nested: { a: [1, null, "x"] } });
  assert.deepEqual(await key.getSettings(), { n: 2, nested: { a: [1, null, "x"] } });
  key.setResources({ sound: "/b.wav" });
  assert.deepEqual(await key.getResources(), { sound: "/b.wav" });
  key.sendToPropertyInspector({ status: "ok" });
  plugin.setGlobalSettings({ token: "x" });
  assert.deepEqual(await plugin.getGlobalSettings(), { token: "x" });
  plugin.openUrl("https://example.com/");
  plugin.log("hello");
  plugin.switchToProfile("DEVICE1", "Profiles/Main");
  plugin.switchToProfile("DEVICE1", "Profiles/Main", 2);
  plugin.switchToProfile("DEVICE1");

  const sent = new Set(host.commands.map((command) => command.event));
  assert.equal(sent.size, 15, [...sent].join(", "));
  assertAllValid();
});

test("every event the mock host sends matches the schemas", async () => {
  const host = new MockHost({ globalSettings: { token: "x" } });
  const { options, assertAllValid } = checked(host);
  const plugin = new Plugin();
  const seen: string[] = [];
  plugin.action(ACTION, {
    willAppear: () => seen.push("willAppear"),
    willDisappear: () => seen.push("willDisappear"),
    keyDown: () => seen.push("keyDown"),
    keyUp: () => seen.push("keyUp"),
    didReceiveSettings: () => seen.push("didReceiveSettings"),
    sendToPlugin: () => seen.push("sendToPlugin"),
  });
  await plugin.connect(options);

  const onKey = { action: ACTION, settings: { n: 1 }, state: 1 };
  host.willAppear(onKey);
  host.press(onKey);
  host.inspectorSetSettings(onKey, { n: 2 });
  host.inspectorSendToPlugin(onKey, { ask: ["a", 1] });
  host.willDisappear(onKey);
  // A step of a Multi Action, which has no coordinates and asks for a state.
  const step = { action: ACTION, context: "context-2", coordinates: null, state: 0 };
  host.willAppear(step);
  host.press(step);
  await new Promise((resolve) => setImmediate(resolve));
  // Answers to requests.
  plugin.action(ACTION, {
    willAppear(_event, key) {
      void key.getSettings();
      void key.getResources();
    },
  });
  host.willAppear({ ...step, context: "context-3" });
  await plugin.getGlobalSettings();
  await new Promise((resolve) => setImmediate(resolve));

  assert.deepEqual(seen, [
    "willAppear",
    "keyDown",
    "keyUp",
    "didReceiveSettings",
    "sendToPlugin",
    "willDisappear",
    "willAppear",
    "keyDown",
    "keyUp",
  ]);
  assertAllValid();
});

test("a message the schemas reject is reported", async () => {
  const host = new MockHost();
  const { options, assertAllValid } = checked(host);
  const plugin = new Plugin();
  await plugin.connect(options);
  plugin.send({ event: "setState", context: "context-1", payload: { state: -1 } });
  assert.throws(assertAllValid, /plugin-commands\.schema\.json rejects/);
});
