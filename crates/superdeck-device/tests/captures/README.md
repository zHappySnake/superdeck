# Protocol captures

Recorded HID traffic, one file per scenario, replayed by `tests/captures.rs`
against the driver. The format is described in
[`src/capture.rs`](../../src/capture.rs).

Every file says where it came from on its first lines:

- **synthetic**: written by hand from the protocol notes in
  [`docs/device-protocol.md`](../../../../docs/device-protocol.md). These pin
  down the protocol as documented, but haven't been checked against hardware.
- **recorded**: captured from a real device with `superdeck-cli --record`.
  The header names the model, firmware, and who recorded it.

## Adding a recording

If you own a device, recordings are one of the most useful contributions:

```sh
superdeck-cli --record mk2-session.cap info
superdeck-cli --record mk2-brightness.cap brightness 50
superdeck-cli --record mk2-events.cap events   # press a few keys, then Ctrl+C
```

Check the result (serial numbers are in there; replace them with a dummy of
the same length if you prefer), add a header comment, and open a pull request.
