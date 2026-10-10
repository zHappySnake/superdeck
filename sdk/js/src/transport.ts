// SPDX-License-Identifier: MIT OR Apache-2.0

/** A connection to the host that carries JSON text messages. */
export interface Transport {
  /** Sends one message. */
  send(text: string): void;
  /** Calls `listener` with each message received. */
  onMessage(listener: (text: string) => void): void;
  /** Calls `listener` once the connection is closed, from either side. */
  onClose(listener: () => void): void;
  /** Closes the connection. */
  close(): void;
}

/** Opens a WebSocket connection with Node.js's built-in client. */
export function webSocketTransport(url: string): Promise<Transport> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const messages: ((text: string) => void)[] = [];
    const closes: (() => void)[] = [];
    socket.addEventListener("message", (event) => {
      const text = typeof event.data === "string" ? event.data : String(event.data);
      for (const listener of messages) listener(text);
    });
    socket.addEventListener("close", () => {
      for (const listener of closes) listener();
    });
    socket.addEventListener("error", () => reject(new Error(`could not connect to ${url}`)), {
      once: true,
    });
    socket.addEventListener(
      "open",
      () =>
        resolve({
          send: (text) => socket.send(text),
          onMessage: (listener) => messages.push(listener),
          onClose: (listener) => closes.push(listener),
          close: () => socket.close(),
        }),
      { once: true },
    );
  });
}
