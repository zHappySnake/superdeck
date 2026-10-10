# Device protocol notes

How SuperDeck talks to Stream Deck hardware over USB HID. This is the
reference for `crates/superdeck-device`; when the code and this file
disagree, one of them has a bug.

## Sources

These notes come from public, openly licensed work, never from Elgato's
software (see the clean-room policy in [CONTRIBUTING.md](../CONTRIBUTING.md)):

- [python-elgato-streamdeck](https://github.com/abcminiuser/python-elgato-streamdeck)
  (MIT), the most complete public description of the protocol
- [elgato-streamdeck](https://crates.io/crates/elgato-streamdeck) (MPL-2.0),
  another public implementation of the same protocol
- USB captures of our own devices, as they come in (see
  [Verification status](#verification-status))

SuperDeck's implementation is our own; no code was copied from either project.

## Conventions

- All devices use USB vendor ID `0x0fd9`.
- Every report starts with its report ID, as hidapi expects.
- SuperDeck numbers keys row by row from the top left, starting at 0. The
  driver translates to the device's numbering where it differs.
- Byte sequences below are hex. `00…` means zero padding up to the report
  length.

## Protocol generations

There are three variants. Every model uses one of them.

|                     | Original       | Mini                      | Gen 2           |
| ------------------- | -------------- | ------------------------- | --------------- |
| Image report length | 8191           | 1024                      | 1024            |
| Image header length | 16             | 16                        | 8               |
| Image format        | BMP            | BMP                       | JPEG            |
| Feature report len  | 17             | 17                        | 32              |
| Input header length | 1              | 1                         | 4               |
| Device key order    | right to left  | left to right             | left to right   |
| Models              | Stream Deck v1 | Mini, Mini MK.2, Module 6 | everything else |

### Key images (output report `02`)

An encoded image is split into pages that each fit in one output report,
padded with zeros to the full report length.

**Original and Mini**, 16-byte header:

| Byte | Meaning                                                           |
| ---- | ----------------------------------------------------------------- |
| 0    | `02` report ID                                                    |
| 1    | `01`                                                              |
| 2    | page number: counts from **1** on the Original, **0** on the Mini |
| 3    | `00`                                                              |
| 4    | `01` on the last page, else `00`                                  |
| 5    | device key index **+ 1**                                          |
| 6–15 | `00`                                                              |

A 72×72 BMP for the Original is 15 606 bytes, so it always takes two pages.

**Gen 2**, 8-byte header:

| Byte | Meaning                                    |
| ---- | ------------------------------------------ |
| 0    | `02` report ID                             |
| 1    | `07` (set key image)                       |
| 2    | key index (0-based)                        |
| 3    | `01` on the last page, else `00`           |
| 4–5  | payload length in this page, little-endian |
| 6–7  | page number from 0, little-endian          |

When the device is opened, the driver sends one all-zero image report
(`02 00…`), which discards a transfer a previous program left unfinished.

### Feature reports

| Command            | Original / Mini                 | Gen 2                      |
| ------------------ | ------------------------------- | -------------------------- |
| Brightness (0–100) | send `05 55 aa d1 01 <pct> 00…` | send `03 08 <pct> 00…`     |
| Reset              | send `0b 63 00…`                | send `03 02 00…`           |
| Serial number      | get `03`, string at byte 5      | get `06`, string at byte 2 |
| Firmware version   | get `04`, string at byte 5      | get `05`, string at byte 6 |

Strings are ASCII, terminated by the first `00`.

Reset shows the boot logo and clears every key image.

### Key states (input report `01`)

The device sends a report whenever a key changes. After the header comes one
byte per key, `01` if pressed, in the device's key order:

- Original / Mini: `01 <state per key>`
- Gen 2: `01 00 <key count, little-endian u16> <state per key>`

The driver compares each report with the previous one to produce key down
and key up events.

The Neo appends two bytes for its touch keys after the eight key states.
SuperDeck reads them but ignores them until touch keys are supported.

### Device key order

The Original numbers keys right to left within each row. The driver maps
SuperDeck's index `k` to the device's with
`k - (k mod 5) + (4 - k mod 5)` (its own inverse), both for images and for
key states. All other models use SuperDeck's order directly.

## Image orientation

Images must be rotated or mirrored before encoding so they appear upright.
The transform is applied to an upright image: rotate counter-clockwise
first, then mirror.

| Models              | Rotation | Mirror                  | Net effect   |
| ------------------- | -------- | ----------------------- | ------------ |
| Original, all Gen 2 | none     | horizontal and vertical | rotated 180° |
| Mini family         | 90°      | vertical                | transposed   |

BMP images are 24-bit, bottom-up, with a 54-byte header (40-byte
`BITMAPINFOHEADER`) and 3780 pixels per meter. JPEG images are baseline,
quality 95.

Images of the wrong size are scaled to fit the key, keeping their aspect
ratio, and centered on black. Transparent pixels become black.

## Models

| Model                      | Product ID | Protocol | Keys       | Key size | `superdeck-cli` id |
| -------------------------- | ---------- | -------- | ---------- | -------- | ------------------ |
| Stream Deck (original)     | `0060`     | Original | 15 (5 × 3) | 72 px    | `original`         |
| Stream Deck (v2)           | `006d`     | Gen 2    | 15 (5 × 3) | 72 px    | `original-v2`      |
| Stream Deck MK.2           | `0080`     | Gen 2    | 15 (5 × 3) | 72 px    | `mk2`              |
| Stream Deck MK.2 (scissor) | `00a5`     | Gen 2    | 15 (5 × 3) | 72 px    | `mk2-scissor`      |
| Stream Deck Mini           | `0063`     | Mini     | 6 (3 × 2)  | 80 px    | `mini`             |
| Stream Deck Mini MK.2      | `0090`     | Mini     | 6 (3 × 2)  | 80 px    | `mini-mk2`         |
| Stream Deck XL             | `006c`     | Gen 2    | 32 (8 × 4) | 96 px    | `xl`               |
| Stream Deck XL (v2)        | `008f`     | Gen 2    | 32 (8 × 4) | 96 px    | `xl-v2`            |
| Stream Deck Neo            | `009a`     | Gen 2    | 8 (4 × 2)  | 96 px    | `neo`              |
| Stream Deck Module 6       | `00b8`     | Mini     | 6 (3 × 2)  | 80 px    | `module-6`         |
| Stream Deck Module 15      | `00b9`     | Gen 2    | 15 (5 × 3) | 72 px    | `module-15`        |
| Stream Deck Module 32      | `00ba`     | Gen 2    | 32 (8 × 4) | 96 px    | `module-32`        |

Not supported yet (after 1.0): Stream Deck + (`0084`), Pedal (`0086`),
Studio (`00aa`), and the Neo's info bar and touch keys.

## Verification status

No model has been checked against real hardware by the SuperDeck project
yet. The protocol tests in `crates/superdeck-device/tests/` replay
**synthetic** captures written from this document; they keep the driver
consistent with these notes but can't prove the notes right.

The Module 6/15/32 entries are the least certain: they assume the modules
behave like the Mini MK.2, MK.2, and XL (v2) respectively.

To verify a model, with the device plugged in and the udev rule installed:

1. `superdeck-cli info` shows the right model, serial number, and firmware.
2. `superdeck-cli test-pattern`: on every key, the white square is in the
   top-left corner and the white bar along the top edge; the key colors run
   red → yellow → green → cyan → blue → magenta row by row from the top
   left.
3. `superdeck-cli events`: pressing each key in turn, left to right and top
   to bottom, prints keys 0, 1, 2, … in order.
4. `superdeck-cli brightness 10`, then `brightness 100`, `reset`.
5. Record the session with `--record` and contribute the capture (see
   `crates/superdeck-device/tests/captures/README.md`).

Then update the table below in the same pull request.

| Model | Verified by | Firmware | Notes |
| ----- | ----------- | -------- | ----- |
| –     |             |          |       |
