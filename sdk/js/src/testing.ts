// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * A mock host for testing plugins without SuperDeck:
 *
 * ```ts
 * const host = new MockHost();
 * await plugin.connect(host.options());
 * host.keyDown("com.example.hello.say");
 * assert.deepEqual(await host.nextCommand(), {
 *   event: "setTitle", context: "context-1", payload: { title: "Hello" },
 * });
 * ```
 *
 * Like the real host, it keeps instance and global settings, and answers
 * `getSettings`, `getResources`, and `getGlobalSettings` itself.
 */

import type { ConnectOptions } from "./plugin.ts";
import type {
  ActionPayload,
  Coordinates,
  DeviceInfo,
  JsonValue,
  PluginCommand,
  PluginEvent,
  RegistrationInfo,
  Resources,
  Settings,
} from "./protocol.ts";
import type { Transport } from "./transport.ts";

/** An instance as {@link MockHost} sends it. */
export interface MockInstance {
  /** The action's UUID. */
  action: string;
  /** Defaults to `context-1`. */
  context?: string;
  /** Defaults to the first device. */
  device?: string;
  settings?: Settings;
  /** Defaults to the top-left key; `null` for a step of a Multi Action. */
  coordinates?: Coordinates | null;
  state?: number;
  resources?: Resources;
}

/** Options of {@link MockHost}. */
export interface MockHostOptions {
  /** The plugin's UUID and version. */
  plugin?: { uuid: string; version: string };
  /** Devices, by ID. One Stream Deck MK.2 by default. */
  devices?: Record<string, DeviceInfo>;
  /** Global settings the plugin starts with. */
  globalSettings?: Settings;
}

const TOKEN = "mock-plugin-token";

export class MockHost {
  readonly #info: RegistrationInfo;
  readonly #toPlugin: ((text: string) => void)[] = [];
  readonly #closes: (() => void)[] = [];
  readonly #waiting: ((command: PluginCommand) => void)[] = [];
  readonly #queue: PluginCommand[] = [];
  #registered = false;
  /** The action of each context sent. */
  readonly #actions = new Map<string, string>();
  /** Every command the plugin sent, in order. */
  readonly commands: PluginCommand[] = [];
  /** Instance settings, by context. */
  readonly settings = new Map<string, Settings>();
  /** Instance resources, by context. */
  readonly resources = new Map<string, Resources>();
  /** The plugin's global settings. */
  globalSettings: Settings;
  /** Messages the plugin wrote with `logMessage`. */
  readonly log: string[] = [];

  constructor(options: MockHostOptions = {}) {
    const devices = options.devices ?? {
      DEVICE1: { name: "Stream Deck MK.2", size: { columns: 5, rows: 3 }, type: 0 },
    };
    this.globalSettings = options.globalSettings ?? {};
    this.#info = {
      application: {
        font: "Noto Sans",
        language: "en",
        platform: "linux",
        platformVersion: "6.12.0",
        version: "7.1.0",
      },
      plugin: options.plugin ?? { uuid: "com.example.plugin", version: "1.0.0.0" },
      devicePixelRatio: 1,
      colors: {
        buttonMouseOverBackgroundColor: "#464646",
        buttonPressedBackgroundColor: "#303030",
        buttonPressedBorderColor: "#646464",
        buttonPressedTextColor: "#969696",
        highlightColor: "#0078ff",
      },
      devices: Object.entries(devices).map(([id, device]) => ({ id, ...device })),
    };
  }

  /** What to pass to `plugin.connect()`. */
  options(): ConnectOptions {
    const argv = ["-port", "1", "-pluginUUID", TOKEN, "-registerEvent", "registerPlugin"];
    return { argv: [...argv, "-info", JSON.stringify(this.#info)], transport: this.#transport() };
  }

  #transport(): Transport {
    return {
      send: (text) => this.#receive(text),
      onMessage: (listener) => this.#toPlugin.push(listener),
      onClose: (listener) => this.#closes.push(listener),
      close: () => {
        for (const listener of this.#closes) listener();
      },
    };
  }

  /** Whether the plugin has registered. */
  get registered(): boolean {
    return this.#registered;
  }

  /** Sends the plugin an event. Delivered asynchronously, as over a socket. */
  send(event: PluginEvent): void {
    const text = JSON.stringify(event);
    queueMicrotask(() => {
      for (const listener of this.#toPlugin) listener(text);
    });
  }

  /** The next command the plugin sends, waiting for it. Commands are handed out once each. */
  nextCommand(timeoutMs = 1000): Promise<PluginCommand> {
    const queued = this.#queue.shift();
    if (queued) return Promise.resolve(queued);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#waiting.splice(this.#waiting.indexOf(waiter), 1);
        reject(new Error("the plugin sent no command"));
      }, timeoutMs);
      const waiter = (command: PluginCommand) => {
        clearTimeout(timer);
        resolve(command);
      };
      this.#waiting.push(waiter);
    });
  }

  /** Sends `willAppear` for an instance. */
  willAppear(instance: MockInstance): void {
    this.send({ event: "willAppear", ...this.#about(instance, false) });
  }

  /** Sends `willDisappear` for an instance. */
  willDisappear(instance: MockInstance): void {
    this.send({ event: "willDisappear", ...this.#about(instance, false) });
  }

  /** Sends `keyDown` for an instance. */
  keyDown(instance: MockInstance): void {
    this.send({ event: "keyDown", ...this.#about(instance, true) });
  }

  /** Sends `keyUp` for an instance. */
  keyUp(instance: MockInstance): void {
    this.send({ event: "keyUp", ...this.#about(instance, true) });
  }

  /** Presses and releases a key. */
  press(instance: MockInstance): void {
    this.keyDown(instance);
    this.keyUp(instance);
  }

  /** Sends what the property inspector would: new settings, saved and passed on. */
  inspectorSetSettings(instance: MockInstance, settings: Settings): void {
    const context = instance.context ?? "context-1";
    this.settings.set(context, settings);
    this.send({ event: "didReceiveSettings", ...this.#about({ ...instance, settings }, false) });
  }

  /** Sends a message from the property inspector. */
  inspectorSendToPlugin(instance: MockInstance, payload: JsonValue): void {
    this.send({
      event: "sendToPlugin",
      action: instance.action,
      context: instance.context ?? "context-1",
      payload,
    });
  }

  #about(instance: MockInstance, keyEvent: boolean) {
    const context = instance.context ?? "context-1";
    this.#actions.set(context, instance.action);
    if (instance.settings) this.settings.set(context, instance.settings);
    if (instance.resources) this.resources.set(context, instance.resources);
    const coordinates =
      instance.coordinates === undefined ? { column: 0, row: 0 } : instance.coordinates;
    const payload: ActionPayload = {
      settings: this.settings.get(context) ?? {},
      controller: "Keypad",
      isInMultiAction: coordinates === null,
      resources: this.resources.get(context) ?? {},
    };
    if (coordinates) payload.coordinates = coordinates;
    if (instance.state !== undefined) {
      payload.state = instance.state;
      if (keyEvent && coordinates === null) payload.userDesiredState = instance.state;
    }
    return {
      action: instance.action,
      context,
      device: instance.device ?? this.#info.devices[0]?.id ?? "",
      payload,
    };
  }

  #receive(text: string): void {
    const message = JSON.parse(text) as PluginCommand | { event: string; uuid: string };
    if (!this.#registered) {
      if (message.event !== "registerPlugin" || !("uuid" in message) || message.uuid !== TOKEN) {
        throw new Error(`the plugin registered wrongly: ${text}`);
      }
      this.#registered = true;
      for (const device of this.#info.devices) {
        const { id, ...deviceInfo } = device;
        this.send({ event: "deviceDidConnect", device: id, deviceInfo });
      }
      return;
    }
    const command = message as PluginCommand;
    this.commands.push(command);
    this.#answer(command);
    const waiter = this.#waiting.shift();
    if (waiter) waiter(command);
    else this.#queue.push(command);
  }

  #answer(command: PluginCommand): void {
    const id = "id" in command && command.id !== undefined ? { id: command.id } : {};
    switch (command.event) {
      case "setSettings":
        this.settings.set(command.context, command.payload);
        break;
      case "setResources":
        this.resources.set(command.context, command.payload);
        break;
      case "setGlobalSettings":
        this.globalSettings = command.payload;
        break;
      case "getGlobalSettings":
        this.send({
          event: "didReceiveGlobalSettings",
          payload: { settings: this.globalSettings },
          ...id,
        });
        break;
      case "getSettings":
      case "getResources": {
        const action = this.#actions.get(command.context) ?? "";
        const event =
          command.event === "getSettings" ? "didReceiveSettings" : "didReceiveResources";
        this.send({ event, ...this.#about({ action, context: command.context }, false), ...id });
        break;
      }
      case "logMessage":
        this.log.push(command.payload.message);
        break;
    }
  }
}
