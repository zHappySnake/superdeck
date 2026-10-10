// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * The messages of the plugin protocol, as in `sdk/protocol/schemas/`. The
 * tests check these types against the schemas.
 */

/** Any JSON value. */
export type JsonValue =
  string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

/** A JSON object. */
export type JsonObject = { [key: string]: JsonValue };

/** Settings of an action instance or a plugin: any JSON object. */
export type Settings = JsonObject;

/** Files an action instance refers to: names mapped to paths. */
export type Resources = { [name: string]: string };

/** A key position, counted from 0 at the top left. */
export interface Coordinates {
  column: number;
  row: number;
}

/** Key grid of a device. */
export interface Size {
  columns: number;
  rows: number;
}

/** A device. `type` is the SDK's device type: 0 Stream Deck, 1 Mini, 2 XL, 9 Neo. */
export interface DeviceInfo {
  name: string;
  size: Size;
  type: number;
}

/** The `-info` argument: about the app, the plugin, and the devices. */
export interface RegistrationInfo {
  application: {
    font: string;
    language: string;
    platform: string;
    platformVersion: string;
    /** The version of the official app whose protocol the host implements. */
    version: string;
  };
  plugin: { uuid: string; version: string };
  devicePixelRatio: number;
  colors: {
    buttonMouseOverBackgroundColor: string;
    buttonPressedBackgroundColor: string;
    buttonPressedBorderColor: string;
    buttonPressedTextColor: string;
    highlightColor: string;
  };
  devices: (DeviceInfo & { id: string })[];
}

/** What an event says about an action instance. */
export interface ActionPayload<S extends Settings = Settings> {
  settings: S;
  /** The key; absent for a step of a Multi Action. */
  coordinates?: Coordinates;
  controller: "Keypad";
  isInMultiAction: boolean;
  /** The state, for actions with two states. */
  state?: number;
  /** In a Multi Action's `keyDown` and `keyUp`: the state the user chose for the step. */
  userDesiredState?: number;
  resources: Resources;
}

/** An event about an action instance. */
export interface ActionEvent<E extends string, S extends Settings = Settings> {
  event: E;
  action: string;
  context: string;
  device: string;
  payload: ActionPayload<S>;
}

export type WillAppear<S extends Settings = Settings> = ActionEvent<"willAppear", S>;
export type WillDisappear<S extends Settings = Settings> = ActionEvent<"willDisappear", S>;
export type KeyDown<S extends Settings = Settings> = ActionEvent<"keyDown", S>;
export type KeyUp<S extends Settings = Settings> = ActionEvent<"keyUp", S>;

/** An instance's settings: the answer to `getSettings` (with its `id`), or settings saved on the other side. */
export interface DidReceiveSettings<S extends Settings = Settings> extends ActionEvent<
  "didReceiveSettings",
  S
> {
  id?: string;
}

/** An instance's resources: the answer to `getResources`, or resources saved on the other side. */
export interface DidReceiveResources<S extends Settings = Settings> extends ActionEvent<
  "didReceiveResources",
  S
> {
  id?: string;
}

/** How a title is drawn. */
export interface TitleParameters {
  fontFamily: string;
  fontSize: number;
  fontStyle: "" | "Regular" | "Bold" | "Italic" | "Bold Italic";
  fontUnderline: boolean;
  showTitle: boolean;
  titleAlignment: "top" | "middle" | "bottom";
  titleColor: string;
}

export interface TitleParametersDidChange<S extends Settings = Settings> {
  event: "titleParametersDidChange";
  action: string;
  context: string;
  device: string;
  payload: {
    settings: S;
    coordinates: Coordinates;
    controller: "Keypad";
    state?: number;
    title: string;
    titleParameters: TitleParameters;
    resources: Resources;
  };
}

export interface DeviceDidConnect {
  event: "deviceDidConnect";
  device: string;
  deviceInfo: DeviceInfo;
}

export interface DeviceDidDisconnect {
  event: "deviceDidDisconnect";
  device: string;
}

export interface DidReceiveGlobalSettings<S extends Settings = Settings> {
  event: "didReceiveGlobalSettings";
  payload: { settings: S };
  id?: string;
}

export interface PropertyInspectorDidAppear {
  event: "propertyInspectorDidAppear";
  action: string;
  context: string;
  device: string;
}

export interface PropertyInspectorDidDisappear {
  event: "propertyInspectorDidDisappear";
  action: string;
  context: string;
  device: string;
}

/** A message from an instance's property inspector. */
export interface SendToPlugin<P extends JsonValue = JsonValue> {
  event: "sendToPlugin";
  action: string;
  context: string;
  payload: P;
}

export interface SystemDidWakeUp {
  event: "systemDidWakeUp";
}

/** Every event a plugin receives. */
export type PluginEvent =
  | DeviceDidConnect
  | DeviceDidDisconnect
  | WillAppear
  | WillDisappear
  | KeyDown
  | KeyUp
  | DidReceiveSettings
  | DidReceiveResources
  | DidReceiveGlobalSettings
  | TitleParametersDidChange
  | PropertyInspectorDidAppear
  | PropertyInspectorDidDisappear
  | SendToPlugin
  | SystemDidWakeUp;

/** The event named `E`. */
export type EventNamed<E extends PluginEvent["event"]> = Extract<PluginEvent, { event: E }>;

/** Every command a plugin may send after registering. */
export type PluginCommand =
  | { event: "setSettings"; context: string; payload: Settings }
  | { event: "getSettings"; context: string; id?: string }
  | { event: "setGlobalSettings"; context: string; payload: Settings }
  | { event: "getGlobalSettings"; context: string; id?: string }
  | { event: "setResources"; context: string; payload: Resources }
  | { event: "getResources"; context: string; id?: string }
  | { event: "openUrl"; payload: { url: string } }
  | { event: "logMessage"; payload: { message: string } }
  | {
      event: "setTitle";
      context: string;
      payload: { title?: string; state?: number; target?: 0 | 1 | 2 };
    }
  | {
      event: "setImage";
      context: string;
      payload: { image?: string; state?: number; target?: 0 | 1 | 2 };
    }
  | { event: "setState"; context: string; payload: { state: number } }
  | { event: "showAlert"; context: string }
  | { event: "showOk"; context: string }
  | {
      event: "switchToProfile";
      context: string;
      device: string;
      payload: { profile?: string; page?: number };
    }
  | { event: "sendToPropertyInspector"; context: string; payload: JsonValue };

/** The first message on a connection. */
export interface Registration {
  event: string;
  uuid: string;
}
