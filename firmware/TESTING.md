# Testing & Validation Playbook

How to build, flash, and validate the MIDI Captain Remedy firmware — the
automated self-tests, the on-device manual checks, and the host tooling that
makes the device observable. Written so a new contributor can take a board from
"freshly cloned repo" to "feature validated and ready to merge."

> **TL;DR for a PR:** run the [gate](#1-the-pre-pr-gate), flash the relevant
> [self-tests](#3-automated-self-tests) (they must print `ALL PASS`), then do the
> [manual / feature checks](#6-on-device-manual-validation) the change touches.
> Copy the [pre-merge checklist](#10-pre-merge-checklist) into the PR.

## Contents

1. [The pre-PR gate](#1-the-pre-pr-gate)
2. [Prerequisites & connections](#2-prerequisites--connections)
3. [Automated self-tests](#3-automated-self-tests)
4. [Flashing](#4-flashing)
5. [Booting the firmware: expected output](#5-booting-the-firmware-expected-output)
6. [On-device manual validation](#6-on-device-manual-validation)
7. [Config sync over USB-CDC](#7-config-sync-over-usb-cdc)
8. [Watching the MIDI output](#8-watching-the-midi-output)
9. [Feature validation matrix](#9-feature-validation-matrix)
10. [Pre-merge checklist](#10-pre-merge-checklist)
11. [Troubleshooting & known gotchas](#11-troubleshooting--known-gotchas)

---

## 1. The pre-PR gate

This is the green-light check every change must pass. It compiles **and** lints
the library, all binaries, and all examples with warnings denied:

```bash
cargo clippy --release --lib --bins --examples -- -D warnings
```

- **Do not use `--all-targets`.** It pulls in the `test` harness target, which
  can't link in `no_std` (`can't find crate for test` / missing `#[panic_handler]`).
  This crate has no `cargo test`; tests are the `examples/*_selftest.rs` (run on
  hardware) plus host-side Python checks.
- A clean exit (`0`) with `Finished` is the gate passing. If you run it through a
  pipe or background shell, read the cargo `Finished`/`error` line — a chained
  shell can report the *shell's* exit, not cargo's.

Optionally build the artifacts too (useful before flashing):

```bash
cargo build --release --bins --examples
```

---

## 2. Prerequisites & connections

### Tools

| Tool | Install | Used for |
|------|---------|----------|
| Rust + target | `rustup target add thumbv6m-none-eabi` | building |
| `probe-rs` | `cargo install probe-rs-tools` | flash + RTT logs (with an SWD probe) |
| `elf2uf2-rs` | `cargo install elf2uf2-rs` | flash via UF2 (no probe needed) |
| `pyserial` | `pip install pyserial` | CDC config client, port listing |
| `midimon` | `cargo install --git https://github.com/sourcebox/midimon` | watch emitted MIDI |

### The two USB connections (this trips everyone up once)

There are **two independent USB links**, and they expose different things:

| Connection | VID:PID | Typical port | Gives you |
|------------|---------|--------------|-----------|
| **SWD debug probe** (Pi Debug Probe / picoprobe) | `2E8A:000C` (CMSIS-DAP) | e.g. `COM8` | `probe-rs` flashing + RTT/`defmt` logs |
| **Device's own USB-C** | `2E8A:102D` (`SER=RMDY-DEV-0001`) | e.g. `COM9` | USB-MIDI + the **CDC config-sync** port |

The probe alone is enough to flash and read RTT. But the **CDC config port and
USB-MIDI only appear when the device's own USB-C is plugged into the host** — it's
the composite USB the firmware itself drives. If `cdc_config_client.py` or
`midimon` can't find the device, that cable is the usual culprit (and make sure
it's a **data** cable, not charge-only).

List ports and identify them:

```powershell
python -m serial.tools.list_ports -v          # all ports + VID:PID
python -m serial.tools.list_ports -q 102D     # just the device's CDC port
```

> **Windows only for anything touching USB.** WSL cannot see USB serial or USB
> MIDI. Build can happen anywhere; run device-facing commands from Windows
> PowerShell.

---

## 3. Automated self-tests

These live in `examples/` and assert on real silicon via `defmt::assert!` — a
failure **panics** through `panic-probe` (you'll see the panic in RTT); success
prints `… ALL PASS` and then idles with a heartbeat so the RTT session stays
attached. Run one with (probe required — see [Flashing](#4-flashing)):

```powershell
cargo run --release --example <name>
```

### Self-contained (no external gear — just the board + probe)

| Example | Validates | Look for |
|---------|-----------|----------|
| `config_selftest` | `RuntimeConfig` RAM + flash round-trip; factory-reset → default fallback | `config self-test: ALL PASS` |
| `storage_coexist_selftest` | settings + config coexist in one flash map (the buffer-sizing fix); worst-case blob ≤ `MAX_SERIALIZED_LEN`; **cleans up to defaults at the end** | `worst-case config + settings coexist OK`, `store erased -- device left at baked default`, `ALL PASS` |
| `proto_selftest` | COBS + CRC-16 framing round-trip at max payload | `proto self-test: round-trip OK` |
| `pitch_selftest` | fixed-point YIN pitch detector vs a Python-mirrored golden | `pitch self-test: ALL PASS` |

> **Important — self-tests that write flash leave state behind.** Both
> `config_selftest` and `storage_coexist_selftest` now **factory-reset at the
> end**, so they leave the device at defaults. Historically `storage_coexist`
> did *not*, and it left a worst-case 8-page config persisted — booting the real
> firmware afterwards loaded that garbage (every label `XXXX`, no page nav). If
> you ever see `XXXX` labels on the display, the flash holds a stale test config:
> [factory-reset it](#factory-reset-recover-from-a-bad-config-or-settings).

### Interactive / hardware-exercising

| Example | Exercises |
|---------|-----------|
| `blink`, `leds_test` | onboard LED / WS2812 chain |
| `encoder_test` | rotary encoder + push |
| `expression_test` | expression-pedal ADC (GP27/GP28) |
| `display_splash`, `display_widgets` | ST7789 panel + widgets |
| `midi_engine_test` | MIDI mux (USB+DIN merge/fan-out, thru) |
| `midi_passthrough` | DIN↔USB passthrough |
| `serial_echo` | USB-CDC echo + 1200-baud reset hook |
| `storage_test` | raw settings persistence |

---

## 4. Flashing

The firmware deploys to the RP2040 — no filesystem copy (that's the old
CircuitPython path). Two runners:

### With a probe (canonical — gives RTT logs)

The committed runner is UF2 (so probe-less contributors can build). Flip it to
`probe-rs` for your session via an **environment override** — **do not commit a
change to `.cargo/config.toml`**:

```powershell
$env:CARGO_TARGET_THUMBV6M_NONE_EABI_RUNNER = "probe-rs run --chip RP2040"
cargo run --release --bin midicaptain          # flash + stream RTT
cargo run --release --example config_selftest  # same, for a self-test
```

Reboot the device **without** reflashing (e.g. to test persistence across a power
cycle):

```powershell
probe-rs reset --chip RP2040    # one-shot; exits and frees the probe
```

### Without a probe (UF2)

Hold **BOOTSEL** during power-on (or 1200-baud-touch the CDC port) so the board
mounts as `RPI-RP2`, then the default `elf2uf2-rs -d` runner drops the UF2 on it:

```bash
cargo run --release --bin midicaptain
```

---

## 5. Booting the firmware: expected output

On a clean boot (`cargo run --release --bin midicaptain` with the probe runner),
RTT should show roughly:

```
MIDICaptain app: boot
settings: Settings { midi_channel: 1, display_brightness: …, led_brightness: …, pedal_cal: […] }
config: 3 page(s)
cdc: host connected            (only if the device USB-C is plugged into a host)
midi mux: USB-MIDI connected
app: alive                     (then every ~5 s)
```

- `storage: stored config corrupt; using default` before `config: 3 page(s)` is
  **normal on a blank/factory-reset device** — it means "no valid user config in
  flash, using the baked default." After you push a config over CDC it loads
  cleanly without that line.
- The display shows `booting…` first, then the first page. A display failure logs
  `display init failed (...); running headless` and is **non-fatal** — MIDI, LEDs,
  and switches keep working.

---

## 6. On-device manual validation

The baked default (what you get on a factory-reset device) is three pages:

**Page 0 — `Default`**

| Switch (index) | Label | Short press | Long press | LED |
|---|---|---|---|---|
| SW1–SW4 (0–3) | PRE1–PRE4 | Program Change 0–3 | — | **radio group 1** (white) |
| A (4) | FX1 | CC 80 toggle | — | green, toggle |
| B (5) | FX2 | CC 81 toggle | — | blue, toggle |
| C (6) | LVL | **cycle** CC 82 = 0→64→127 (wraps) | **reset** cycle to 0 | amber; full off base state, dim on base |
| D (7) | FX4 | CC 83 toggle | **Tuner** | purple, toggle |
| UP (8) | BANK+ | Program Change 4 | **Page next** | cyan |
| DOWN (9) | BANK- | Program Change 5 | **Page prev** | cyan |

**Page 1 — `Katana`** (long-press UP/DOWN on page 0 to reach it)

| Switch | Label | Action | LED |
|---|---|---|---|
| SW1–SW3 | CLEAN/CRUNCH/LEAD | SysEx AmpType 1/2/3 | **radio group 1** |
| SW4 | BROWN | SysEx AmpType 4 (long: Tuner) | **radio group 1** |
| A–D | CH1–CH4 | SysEx RecallPreset 1–4 | **radio group 2** |
| UP/DOWN | PAGE+/PAGE- | Page next / prev | cyan |

**Page 2 — `HID`** (long-press UP/DOWN to reach it) — USB-HID keyboard + media
keys to the **host** (the device enumerates a HID interface alongside MIDI + CDC).
Watch the host, not `midimon` — these send HID reports, not MIDI.

| Switch | Label | Sends (HID) | LED |
|---|---|---|---|
| SW1 | SPACE | keyboard `Space` | white |
| SW2 | ENTER | keyboard `Enter` | white |
| SW3 | UNDO | keyboard `Ctrl+Z` | cyan |
| SW4 | REDO | keyboard `Ctrl+Shift+Z` | cyan |
| A | PLAY | consumer `Play/Pause` | green |
| B | VOL+ | consumer `Volume Up` | blue |
| C | VOL- | consumer `Volume Down` | blue |
| D | MUTE | consumer `Mute` | amber |
| UP/DOWN | PAGE+/PAGE- | Page next / prev | purple |

### What to check

- **Display:** page title (`Default` / `Katana` / `HID`) and page index (`1/3`).
  Pressing a button flashes its label on the status line. *No `XXXX` —* if you see
  it, [factory-reset](#factory-reset-recover-from-a-bad-config-or-settings).
- **LEDs:** bound buttons lit at their colour, unbound dark. Toggle buttons (FX1/2/4)
  go full-bright when ON, dim when OFF.
- **Multi-state cycle (LVL, page 0 C):** tap repeatedly → `CC 82` = 0, 64, 127,
  then wraps to 0. LED is full on 64/127 and dim on 0 (the base state). Long-press
  → resets to 0 (emits `CC 82 0`).
- **USB-HID (page 2):** with the device USB-C in a computer, focus a text field and
  tap SPACE/ENTER/UNDO/REDO — the keystrokes (incl. Ctrl+Z / Ctrl+Shift+Z) land on
  the host. Tap PLAY/VOL+/VOL-/MUTE — the host's media keys respond. These produce
  **no MIDI**; verify on the host, not in `midimon`.
- **Page nav:** long-press UP or DOWN cycles pages; toggle/group/cycle state clears
  on page change.
- **Encoder:** turn → CC 7 (volume). **Long-press → settings menu.**
- **Expression pedals:** pedal 1 → CC 1, pedal 2 → CC 7 (see them in
  [midimon](#8-watching-the-midi-output)).

### Settings menu

Long-press the encoder to enter. Rotate to navigate, press to select. Items:
MIDI Channel, Display Brightness, LED Brightness, Expression Pedal Calibration
(3-step wizard: set min → set max → confirm; persists to flash). Long-press the
encoder again to save + exit.

### Tuner

Long-press **D** (page 0) or **BROWN** (page 1) to enter tuner mode — it sends
`CC#25 = 127` to start a connected amp's tuner and shows a pitch readout. Any
footswitch release (or an encoder hold) exits and sends `CC#25 = 0`.

> The readout is driven by inbound **Note On + Pitch Bend**; it only animates with
> a source that streams those. A BOSS Katana does **not** broadcast tuner data
> over MIDI, and the on-device DSP path is hardware-gated (see `HARDWARE.md`). So
> on a stock setup you can verify the *mode transition + CC#25 emit*, not a live
> pitch.

---

## 7. Config sync over USB-CDC

The drive-free config link (COBS + CRC-16 framing, postcard `RuntimeConfig`
blob). Full tool docs: [`scripts/README.md`](scripts/README.md). The client is
the **reference codec** the web editor is ported from — keep them in lockstep.

```powershell
python scripts/cdc_config_client.py COM9 hello       # handshake — prints protocol version
python scripts/cdc_config_client.py COM9 get         # read + pretty-print the live config
python scripts/cdc_config_client.py COM9 roundtrip   # GET→SET→GET, assert unchanged
python scripts/cdc_config_client.py COM9 get --json cfg.json   # save to edit
python scripts/cdc_config_client.py COM9 set cfg.json          # push (.json encodes, .bin raw)
python scripts/cdc_config_client.py COM9 tweak       # rename page 1 + recolor a button (visible hot-reload)
```

What to look for:

- **`hello`** prints the **protocol version** — must match `proto::PROTO_VERSION`
  (currently **8**). A mismatch means the firmware on the board predates the
  feature you're testing — reflash. (v8 = tap tempo; v7 = per-page encoder/expr
  bindings; v6 = CC trigger; v5 = HID actions; v4 = cycles; v3 = groups;
  v2 = MIDI-thru.)
- **`get`** decodes the live config. After a factory reset it shows the 3 baked
  pages with real labels, the radio groups (`PRE1–4 group=1`, amps `group=1`,
  `CH1–4 group=2`), the `LVL` cycle (`cycle 0: 3 step(s) long=reset`), and the
  page-2 HID actions (`{'type': 'hid', ...}`).
- **`set` / `tweak`** persist + **hot-reload**: RTT logs `router: config applied
  (N page(s))` and the screen/LEDs update live without a reboot. The config also
  survives a `probe-rs reset`.
- **`roundtrip`** must report the byte count **identical** before/after — proves
  the codec and flow are lossless.

> **Editing a config by hand:** `get --json cfg.json`, edit, `set cfg.json`. This
> is how you exercise features the baked default doesn't use (momentary CC, PC
> inc/dec, MIDI-thru) — see the [matrix](#9-feature-validation-matrix).

---

## 8. Watching the MIDI output

`midimon` (wrapped by `scripts/midimon.ps1`) shows the MIDI the device actually
emits — the output-side complement to RTT. Full docs in
[`scripts/README.md`](scripts/README.md).

```powershell
./scripts/midimon.ps1 -List          # list input ports
./scripts/midimon.ps1                # stream the device as hex
./scripts/midimon.ps1 -Format min    # one human-ish line per message
```

It resolves the device **by name** (`MIDICaptain Remedy (Rust)`), so a shifting
port id doesn't matter. If it doesn't appear, the device USB-C isn't enumerated
(see [connections](#2-prerequisites--connections)). Default MIDI channel is 1
(channel reverts to 1 after a factory reset).

To read the **Roland/Katana SysEx** in that stream — the boot device-state
sweep and the amp's replies — pipe it through `scripts/sysex_decode.py`:

```powershell
./scripts/midimon.ps1 -Format min-hex | python scripts/sysex_decode.py
```

On a Katana, booting should show the device-sync query and the amp answering:

```text
DT1 set   EditorMode = ENTER  [01]
RQ1 read  AmpType  len=1
DT1 set   AmpType = 3 (Lead)  [03]      # amp's reply → lights the LEAD radio
```

`python scripts/sysex_decode.py --selftest` checks the decoder against
known-good vectors (`9/9 ALL PASS`) — handy offline, with no hardware.

---

## 9. Feature validation matrix

Press the gesture, confirm the MIDI in `midimon` and the LED/display behaviour.
Features marked **(push)** aren't in the baked default — push a config that uses
them via `cdc_config_client.py set`.

| Feature | Gesture | Expect on the wire | Expect on the device |
|---|---|---|---|
| **CC toggle** | tap FX1 (A) | `CC 80 127`, then `CC 80 0` on next tap | LED full ⇄ dim each tap |
| **CC momentary** *(push)* | press & hold a momentary button | `CC 127` on press, `CC 0` on release | status shows on/off with the edges |
| **Program Change** | tap PRE2 (SW2) | `PC 1` | label flashes |
| **PC inc/dec step** *(push)* | tap a `pc_step` button | `PC` = current ± step, clamped 0–127 | label flashes |
| **SysEx (Katana)** | Katana page, tap CLEAN | Roland SysEx (AmpType) w/ valid checksum | label flashes |
| **Page nav** | long-press UP | — | page flips, toggle/group state clears |
| **MIDI-thru routing** *(push)* | enable a route, feed MIDI into the source port | input forwarded to the routed output only | — |
| **Select / radio group** | Page 0: tap PRE1 then PRE2 | each tap sends its `PC` | **lit LED moves**; only one of the group full-bright, rest dim |
| **Two groups, independent** | Katana page: CLEAN→LEAD, then CH1→CH3 | amp SysEx, then channel SysEx | amp group and channel group light independently |
| **Group ≠ latch on long-press** | Katana page: long-press BROWN | `CC#25 127` (enters tuner) | amp selection **unchanged** |
| **Multi-state cycle** | Page 0: tap LVL (C) repeatedly | `CC 82` = 0, 64, 127, wrap | LED dim on base (0), full on 64/127 |
| **Cycle long-press (reset)** | Page 0: long-press LVL | `CC 82 0` | LED returns to dim (base) |
| **USB-HID keyboard** | Page 2: tap SPACE / UNDO | — *(no MIDI)* | host receives `Space` / `Ctrl+Z` keystroke |
| **USB-HID consumer** | Page 2: tap PLAY / VOL+ | — *(no MIDI)* | host's media transport / volume responds |

---

## 10. Pre-merge checklist

```
- [ ] Gate green: cargo clippy --release --lib --bins --examples -- -D warnings
- [ ] Affected self-tests print ALL PASS on hardware
      (config_selftest / storage_coexist_selftest / proto_selftest / pitch_selftest)
- [ ] Manual checks for the touched feature pass (display / LEDs / MIDI via midimon)
- [ ] Config-model change? mirrored in scripts/cdc_config_client.py + PROTO_VERSION bumped if wire-breaking
- [ ] CDC: hello reports the expected proto version; get/roundtrip succeed
- [ ] Cargo.lock committed; .cargo/config.toml runner NOT flipped
- [ ] Device left at a clean state (any flash-writing self-test self-cleans / factory-reset done)
```

---

## 11. Troubleshooting & known gotchas

### Factory reset (recover from a bad config or settings)

Hold the **UP + DOWN** footswitches (scan indices 8 & 9 — the page-nav switches)
**during power-on**. The firmware erases the entire flash KV map (settings *and*
config) and boots the baked default. RTT logs `factory reset: UP+DOWN held at
boot — erasing settings + config`. This is the cure for a pushed config that
breaks the UI, a bad MIDI channel, or stale calibration.

- The combo is read with a 5 ms pull-up settle + a 50 ms held-confirm, so a normal
  boot won't trip it (a transient logs `UP+DOWN transient at power-on … no factory
  reset` and is ignored).

### Display shows `XXXX` labels / pages look identical

The flash holds a stale **worst-case test config** left by an older self-test run.
Factory-reset (above) or push a fresh config.

### `cdc_config_client.py` / `midimon` can't find the device

The device's **own USB-C** isn't enumerated. The debug probe (`2E8A:000C`,
e.g. `COM8`) is *not* the config port — you need `2E8A:102D` (`COM9`). Plug the
device's USB-C into the host with a **data** cable, then re-check with
`python -m serial.tools.list_ports -v`.

### `probe-rs` says "USB device could not be opened" but `probe-rs list` shows it

A stale `probe-rs` process is holding the probe. Kill it and retry:

```powershell
Stop-Process -Name probe-rs
```

`cargo run` can orphan its `probe-rs` child when interrupted — kill that too.

### `hello` reports an unexpected protocol version

The board is running firmware older/newer than your client. Reflash the firmware
that matches the feature under test; `PROTO_VERSION` bumps on every wire-breaking
config-model change.

### Build fails with `can't find crate for test`

You used `--all-targets`. Use `--lib --bins --examples` instead (see the
[gate](#1-the-pre-pr-gate)).
