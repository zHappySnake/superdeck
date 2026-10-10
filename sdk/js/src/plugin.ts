// SPDX-License-Identifier: MIT OR Apache-2.0

import type {
  DeviceInfo,
  DidReceiveResources,
  DidReceiveSettings,
  EventNamed,
  JsonValue,
  KeyDown,
  KeyUp,
  PluginCommand,
  PluginEvent,
  PropertyInspectorDidAppear,
  PropertyInspectorDidDisappear,
  RegistrationInfo,
  Resources,
  SendToPlugin,
  Settings,
  TitleParametersDidChange,
  WillAppear,
  WillDisappear,
} from "./protocol.ts";
import { type Transport, webSocketTransport } from "./transport.ts";

/** How long `getSettings` and friends wait for an answer. */
const REQUEST_TIMEOUT_MS = 15_000;

/** The command-line arguments the host starts a plugin with. */
export interface LaunchArguments {
  port: number;
  pluginUUID: string;
  registerEvent: string;
  info: RegistrationInfo;
}

/** Reads `-port`, `-pluginUUID`, `-registerEvent`, and `-info`. */
export function parseArguments(argv: readonly string[]): LaunchArguments {
  const values = new Map<string, string>();
  for (let i = 0; i < argv.length - 1; i++) {
    const name = argv[i];
    if (
      name === "-port" ||
      name === "-pluginUUID" ||
      name === "-registerEvent" ||
      name === "-info"
    ) {
      values.set(name, argv[++i] ?? "");
    }
  }
  const missing = ["-port", "-pluginUUID", "-registerEvent", "-info"].filter(
    (name) => !values.has(name),
  );
  if (missing.length > 0) {
    throw new Error(
      `missing ${missing.join(", ")}: plugins are started by SuperDeck or the Stream Deck app`,
    );
  }
  const port = Number(values.get("-port"));
  if (!Number.isInteger(port) || port <= 0 || port > 65_535) {
    throw new Error(`invalid -port ${values.get("-port")}`);
  }
  return {
    port,
    pluginUUID: values.get("-pluginUUID") ?? "",
    registerEvent: values.get("-registerEvent") ?? "",
    info: JSON.parse(values.get("-info") ?? "") as RegistrationInfo,
  };
}

/** What an action does when things happen to its instances. Every handler is optional. */
export interface ActionHandlers<S extends Settings = Settings> {
  willAppear?(event: WillAppear<S>, key: Key<S>): unknown;
  willDisappear?(event: WillDisappear<S>, key: Key<S>): unknown;
  keyDown?(event: KeyDown<S>, key: Key<S>): unknown;
  keyUp?(event: KeyUp<S>, key: Key<S>): unknown;
  /** Settings the property inspector saved. Answers to `getSettings` don't come here. */
  didReceiveSettings?(event: DidReceiveSettings<S>, key: Key<S>): unknown;
  /** Resources the property inspector saved. Answers to `getResources` don't come here. */
  didReceiveResources?(event: DidReceiveResources<S>, key: Key<S>): unknown;
  titleParametersDidChange?(event: TitleParametersDidChange<S>, key: Key<S>): unknown;
  propertyInspectorDidAppear?(event: PropertyInspectorDidAppear, key: Key<S>): unknown;
  propertyInspectorDidDisappear?(event: PropertyInspectorDidDisappear, key: Key<S>): unknown;
  /** A message from the property inspector. */
  sendToPlugin?(event: SendToPlugin, key: Key<S>): unknown;
}

/** Options of {@link Plugin.connect}. */
export interface ConnectOptions {
  /** The arguments; `process.argv` by default. */
  argv?: readonly string[];
  /** The connection; a WebSocket to the host by default. Tests pass a mock host's. */
  transport?: Transport;
}

/** One action instance: a key, or a step of a Multi Action. */
export class Key<S extends Settings = Settings> {
  readonly #plugin: Plugin;
  /** The instance's ID; stable across restarts. */
  readonly context: string;
  /** The action's UUID. */
  readonly action: string;
  /** The device it's on. */
  readonly device: string;

  constructor(plugin: Plugin, context: string, action: string, device: string) {
    this.#plugin = plugin;
    this.context = context;
    this.action = action;
    this.device = device;
  }

  /** Shows a title, for one state or both; no title resets it. */
  setTitle(title?: string, state?: number): void {
    this.#plugin.send({
      event: "setTitle",
      context: this.context,
      payload: definedOnly({ title, state }),
    });
  }

  /**
   * Shows an image, for one state or both: a `data:` URL, or a path inside the plugin directory.
   * No image resets it.
   */
  setImage(image?: string, state?: number): void {
    this.#plugin.send({
      event: "setImage",
      context: this.context,
      payload: definedOnly({ image, state }),
    });
  }

  /** Changes the state of a two-state action. */
  setState(state: number): void {
    this.#plugin.send({ event: "setState", context: this.context, payload: { state } });
  }

  /** Shows a warning on the key for a moment. */
  showAlert(): void {
    this.#plugin.send({ event: "showAlert", context: this.context });
  }

  /** Shows a check mark on the key for a moment. */
  showOk(): void {
    this.#plugin.send({ event: "showOk", context: this.context });
  }

  /** Saves the instance's settings, replacing them. */
  setSettings(settings: S): void {
    this.#plugin.send({ event: "setSettings", context: this.context, payload: settings });
  }

  /** The instance's settings. */
  async getSettings(): Promise<S> {
    const answer = await this.#plugin.request("getSettings", "didReceiveSettings", this.context);
    return answer.payload.settings as S;
  }

  /** Saves the instance's resources, replacing them. */
  setResources(resources: Resources): void {
    this.#plugin.send({ event: "setResources", context: this.context, payload: resources });
  }

  /** The instance's resources. */
  async getResources(): Promise<Resources> {
    const answer = await this.#plugin.request("getResources", "didReceiveResources", this.context);
    return answer.payload.resources;
  }

  /** Sends a message to the instance's property inspector, if it's open. */
  sendToPropertyInspector(payload: JsonValue): void {
    this.#plugin.send({ event: "sendToPropertyInspector", context: this.context, payload });
  }
}

/**
 * A plugin. Declare its actions, then connect:
 *
 * ```ts
 * const plugin = new Plugin();
 * plugin.action("com.example.hello.say", {
 *   keyDown(event, key) {
 *     key.setTitle("Hello");
 *   },
 * });
 * await plugin.connect();
 * ```
 */
export class Plugin {
  #transport: Transport | undefined;
  #launch: LaunchArguments | undefined;
  readonly #actions = new Map<string, ActionHandlers<Settings>>();
  readonly #keys = new Map<string, Key<Settings>>();
  readonly #listeners = new Map<string, Set<(event: PluginEvent) => unknown>>();
  readonly #pending = new Map<string, (event: PluginEvent) => void>();
  #nextId = 0;
  /** The connected devices, by ID. */
  readonly devices = new Map<string, DeviceInfo>();

  /** Handles the events of an action's instances. */
  action<S extends Settings = Settings>(uuid: string, handlers: ActionHandlers<S>): this {
    this.#actions.set(uuid, handlers as ActionHandlers<Settings>);
    return this;
  }

  /** Calls `listener` with every event named `event`. Returns a function that stops it. */
  on<E extends PluginEvent["event"]>(
    event: E,
    listener: (event: EventNamed<E>) => unknown,
  ): () => void {
    const listeners = this.#listeners.get(event) ?? new Set();
    this.#listeners.set(event, listeners);
    const wrapped = listener as (event: PluginEvent) => unknown;
    listeners.add(wrapped);
    return () => listeners.delete(wrapped);
  }

  /** Connects to the host and registers. */
  async connect(options: ConnectOptions = {}): Promise<void> {
    if (this.#transport) throw new Error("already connected");
    const launch = parseArguments(options.argv ?? process.argv.slice(2));
    const transport =
      options.transport ?? (await webSocketTransport(`ws://127.0.0.1:${launch.port}`));
    this.#launch = launch;
    this.#transport = transport;
    for (const device of launch.info.devices) {
      this.devices.set(device.id, { name: device.name, size: device.size, type: device.type });
    }
    transport.onMessage((text) => this.#receive(text));
    transport.send(JSON.stringify({ event: launch.registerEvent, uuid: launch.pluginUUID }));
  }

  /** About the host and this plugin, from the `-info` argument. */
  get info(): RegistrationInfo {
    return this.#launched().info;
  }

  /** The plugin's UUID. */
  get uuid(): string {
    return this.#launched().info.plugin.uuid;
  }

  /** Closes the connection. */
  close(): void {
    this.#transport?.close();
  }

  /** Calls `listener` once the host closes the connection. */
  onClose(listener: () => void): void {
    if (!this.#transport) throw new Error("not connected yet");
    this.#transport.onClose(listener);
  }

  /** The plugin's global settings. */
  async getGlobalSettings<S extends Settings = Settings>(): Promise<S> {
    const answer = await this.request(
      "getGlobalSettings",
      "didReceiveGlobalSettings",
      this.#launched().pluginUUID,
    );
    return answer.payload.settings as S;
  }

  /** Saves the plugin's global settings, replacing them. */
  setGlobalSettings(settings: Settings): void {
    this.send({
      event: "setGlobalSettings",
      context: this.#launched().pluginUUID,
      payload: settings,
    });
  }

  /** Opens a URL in the default browser. */
  openUrl(url: string): void {
    this.send({ event: "openUrl", payload: { url } });
  }

  /** Writes a message to the plugin's log. */
  log(message: string): void {
    this.send({ event: "logMessage", payload: { message } });
  }

  /** Switches a device to a profile the plugin ships, or back to the previous one. */
  switchToProfile(device: string, profile?: string, page?: number): void {
    this.send({
      event: "switchToProfile",
      context: this.#launched().pluginUUID,
      device,
      payload: definedOnly({ profile, page }),
    });
  }

  /** Sends a command. */
  send(command: PluginCommand): void {
    if (!this.#transport) throw new Error("not connected yet; call connect() first");
    this.#transport.send(JSON.stringify(command));
  }

  /** Sends a `get…` command and waits for its answer. */
  request<A extends "didReceiveSettings" | "didReceiveResources" | "didReceiveGlobalSettings">(
    command: "getSettings" | "getResources" | "getGlobalSettings",
    answer: A,
    context: string,
  ): Promise<EventNamed<A>> {
    const id = `${this.#nextId++}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`no answer to ${command}`));
      }, REQUEST_TIMEOUT_MS);
      this.#pending.set(id, (event) => {
        if (event.event !== answer) return;
        clearTimeout(timer);
        this.#pending.delete(id);
        resolve(event as EventNamed<A>);
      });
      this.send({ event: command, context, id } as PluginCommand);
    });
  }

  #launched(): LaunchArguments {
    if (!this.#launch) throw new Error("not connected yet; call connect() first");
    return this.#launch;
  }

  #receive(text: string): void {
    let event: PluginEvent;
    try {
      event = JSON.parse(text) as PluginEvent;
    } catch {
      return;
    }
    // Answers to requests go to whoever asked.
    if ("id" in event && typeof event.id === "string") {
      const pending = this.#pending.get(event.id);
      if (pending) {
        pending(event);
        return;
      }
    }
    switch (event.event) {
      case "deviceDidConnect":
        this.devices.set(event.device, event.deviceInfo);
        break;
      case "deviceDidDisconnect":
        this.devices.delete(event.device);
        break;
    }
    if ("action" in event && "context" in event) {
      this.#dispatch(event);
    }
    for (const listener of this.#listeners.get(event.event) ?? []) {
      this.#safely(() => listener(event), event.event);
    }
  }

  #dispatch(event: Extract<PluginEvent, { action: string; context: string }>): void {
    const handlers = this.#actions.get(event.action);
    let key = this.#keys.get(event.context);
    if (!key) {
      const device = "device" in event ? event.device : "";
      key = new Key(this, event.context, event.action, device);
      this.#keys.set(event.context, key);
    }
    if (event.event === "willDisappear") this.#keys.delete(event.context);
    const handler = handlers?.[event.event] as
      ((event: PluginEvent, key: Key) => unknown) | undefined;
    if (handler) this.#safely(() => handler.call(handlers, event, key), event.event);
  }

  /** Runs a handler, logging what it throws or rejects with. */
  #safely(run: () => unknown, what: string): void {
    const report = (error: unknown) => {
      const message = `error in ${what} handler: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}`;
      console.error(message);
      try {
        this.log(message);
      } catch {
        // Not connected any more.
      }
    };
    try {
      const result = run();
      if (result instanceof Promise) result.catch(report);
    } catch (error) {
      report(error);
    }
  }
}

/** The object without its `undefined` fields, which JSON leaves out anyway. */
function definedOnly<T extends object>(object: T): { [K in keyof T]: Exclude<T[K], undefined> } {
  return Object.fromEntries(Object.entries(object).filter(([, value]) => value !== undefined)) as {
    [K in keyof T]: Exclude<T[K], undefined>;
  };
}
