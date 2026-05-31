# Live Device Sync · MIDI Captain (Rust/Embassy firmware)

`live.html` is a Web Serial editor for the **Rust/Embassy firmware's live
configuration**. It connects to the device's USB-CDC port, reads the running
config, lets you edit it visually, and pushes it back — the device persists it
to flash and hot-reloads it with no reboot.

This is **separate** from the OEM file editor (`index.html` + `app.js`), which
builds Paint-Audio preset *files* (`page*.txt` / `gekey*.dat`) for the stock
firmware. The two edit different models, so they live as two tabs/modes rather
than one fused editor (see "OEM vs RuntimeConfig" below).

## Run

Web Serial needs a Chromium browser (Chrome/Edge) and a **secure context** —
`https://` or `localhost`. `file://` will not work (`navigator.serial` is
undefined there). Any static server is fine:

```bash
python -m http.server 5173 --directory webapp   # then open /live.html
# or:
node webapp/test/serve.mjs 5173                  # serves live.html at /
```

Open `http://localhost:5173/live.html`, click **Connect device**, and pick the
port whose USB id is `2E8A:102D` (serial `RMDY-DEV-0001`).

## Flow

1. **Connect** → opens the CDC port and sends `HELLO`; the editor checks the
   device's `PROTO_VERSION` matches (currently `1`).
2. **Read** (automatic on connect, or the Read button) → `GET_CONFIG`, decode
   the postcard blob, load it into the board/inspector.
3. **Edit** → pick a switch, set its label / LED colour / press + long-press
   actions; add/remove/rename pages (max 8).
4. **Write to device** → normalize + validate, then `SET_CONFIG`. The device
   validates, writes flash, and hot-reloads before replying — on success the
   change is already live.

## Architecture

```
webapp/
├── live.html                 entry (two tabs: Live Device | OEM Files)
├── css/style.css             dark, square, mono-for-technical
├── js/
│   ├── app.js                bootstrap + tab routing
│   ├── device/               transport + codec (no DOM; Node-testable)
│   │   ├── varint.js         LEB128
│   │   ├── crc16.js          CRC-16/CCITT-FALSE
│   │   ├── cobs.js           COBS framing
│   │   ├── frame.js          cmd|seq|payload|crc, opcodes
│   │   ├── postcard.js       RuntimeConfig <-> postcard bytes
│   │   ├── runtimeconfig.js  model: limits, defaults, validation, UI metadata
│   │   ├── serial.js         Web Serial transport + frame splitter
│   │   └── protocol.js       CdcLink: hello / getConfig / setConfig
│   └── ui/
│       ├── dom.js            tiny element helper
│       ├── board.js          10-switch board view
│       ├── inspector.js      per-button editor
│       ├── live.js           Live Device controller (wires it all)
│       └── oem.js            OEM tab placeholder (links to index.html)
└── test/
    ├── gen_golden.py         regenerate golden vectors from the reference codec
    ├── golden_vectors.json   captured byte vectors (the spec)
    ├── codec.test.mjs        Node self-test: JS codec == golden bytes
    └── serve.mjs             zero-dep static server for local dev
```

## Wire protocol (source of truth)

The JS codec is a hand port of the hardware-verified host client
`firmware/scripts/cdc_config_client.py`, which mirrors `firmware/src/config/mod.rs`
and `firmware/src/proto.rs`. **Keep all of these in lockstep.**

- **Framing** (`proto.rs`): `COBS(cmd | seq | payload | crc16_be) || 0x00`.
  CRC-16/CCITT-FALSE over `cmd|seq|payload`.
- **Opcodes**: `HELLO=0x01`, `GET_CONFIG=0x02`, `SET_CONFIG=0x03`,
  `REBOOT=0x04` (device replies ERROR — not implemented yet), `ERROR=0xFF`.
  `PROTO_VERSION=1`. ERROR payload `[1=BadCommand|2=BadPayload|3=StoreFailed]`.
- **Config payload** = postcard `RuntimeConfig`:
  - `RuntimeConfig` = `varint(pages.len)` ++ `Page*` (≤ 8 pages)
  - `Page` = `String(name)` ++ `Button[10]` (fixed array, no length prefix)
  - `Button` = `String(label)` ++ `u8 r` ++ `u8 g` ++ `u8 b` ++ `Action` (press)
    ++ `Action` (long-press)
  - `String` = `varint(byteLen)` ++ utf8
  - `Action` = `varint(tag)` ++ fields:
    `none(0)` | `midi_cc(1): u8 cc ++ CcValue` | `program_change(2): u8 program`
    | `sysex(3): varint(SysexCmd) ++ u8 param` | `page_change(4): u8 page`
    | `page_next(5)` | `page_prev(6)` | `tuner_toggle(7)`
  - `CcValue` = `varint(tag)` ++ (`fixed(0): u8 value` | `toggle(1)`)
  - `SysexCmd` = `recall_preset(0) | amp_type(1) | gain(2) | volume(3)`, each `+ u8 param`

> Note: an earlier task brief sketched a simpler `MidiCc{channel,cc,value}` /
> raw-bytes `Sysex` shape. The **firmware** is authoritative; the layout above
> (matching `config/mod.rs`) is what the device actually speaks.

Limits (`config/mod.rs`): `MAX_PAGES=8`, `PAGE_BUTTONS=10`, label `LABEL_CAP=12`
bytes, page name `NAME_CAP=16` bytes. All tags/values currently fit a single
LEB128 byte; the codec implements true multi-byte LEB128 so the format can grow
(more Action variants, more pages) without a rewrite.

## Tests

```bash
cd webapp && npm test          # node test/codec.test.mjs
```

`codec.test.mjs` asserts the JS codec produces byte-identical output to
`golden_vectors.json` (varints, CRC, COBS, full frames, and every config case)
and that decode round-trips. The vectors were generated from the
hardware-verified Python reference; regenerate after any firmware wire change:

```bash
python webapp/test/gen_golden.py --client <path>/cdc_config_client.py
```

(The firmware is a submodule on `main`; point `--client` at a checkout of the
firmware branch where the config-sync work lives.)

## OEM vs RuntimeConfig

The OEM editor models Paint-Audio preset *files* — a different schema, written
by copying files to a USB drive. The Rust firmware exposes a *live* config
object over a serial protocol. Rather than force one model onto the other, this
build keeps them as two tabs:

- **Live Device** — this feature.
- **OEM Files** — links to the existing `index.html` editor.

A future iteration could share UI widgets between them, but the data models stay
distinct.

## Not yet

- `REBOOT` opcode (device returns ERROR; the editor never sends it).
- Importing an OEM preset file into a RuntimeConfig (different model).
- Per-action MIDI channel (the firmware uses one global channel from settings).
