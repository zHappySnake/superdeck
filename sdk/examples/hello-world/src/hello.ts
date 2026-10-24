// SPDX-License-Identifier: MIT OR Apache-2.0

import type { Key, Plugin } from "@superdeck/sdk";

/** The action's UUID, as in `manifest.json`. */
export const SAY_HELLO = "com.example.hello.say";

/** What each "Say Hello" key keeps: its greeting, set in the property inspector, and its count. */
export type HelloSettings = {
  greeting?: string;
  count?: number;
};

/** The key's title: the greeting, and how many times the key was pressed. */
export function title(settings: HelloSettings): string {
  const greeting = settings.greeting?.trim() || "Hello";
  return settings.count ? `${greeting}\n×${settings.count}` : greeting;
}

/** Adds the "Say Hello" action to a plugin. */
export function sayHello(plugin: Plugin): Plugin {
  const show = (settings: HelloSettings, key: Key<HelloSettings>) => key.setTitle(title(settings));
  return plugin.action<HelloSettings>(SAY_HELLO, {
    willAppear: (event, key) => show(event.payload.settings, key),
    // The property inspector changed the greeting.
    didReceiveSettings: (event, key) => show(event.payload.settings, key),
    keyDown(event, key) {
      const settings = {
        ...event.payload.settings,
        count: (event.payload.settings.count ?? 0) + 1,
      };
      key.setSettings(settings);
      show(settings, key);
      key.showOk();
    },
  });
}
