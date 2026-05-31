# MIDICaptain Remedy — Rust + Embassy firmware

Rust port of the MIDI Captain firmware. **Status: subsystems built,
integration next (end of Wave 1).** The ST7789 display is
hardware-validated, and every input/output subsystem now exists as a
self-contained module with a proof example: HAL tasks (encoder,
expression pedals, WS2812 LEDs), the MIDI engine (USB+DIN mux, streaming
SysEx, BOSS Katana/Roland builders), and a flash-backed settings store.
The application binary (`src/bin/midicaptain.rs`) currently runs a
buttons→router→display skeleton; **Wave 2 wires the landed modules into
the router** and builds the config/page action system. See
[`ARCHITECTURE.md`](ARCHITECTURE.md) and [`HANDOFF.md`](HANDOFF.md).

This crate lives alongside the existing CircuitPython firmware in
[`../remedy/`](../remedy/). That code is the **behavioural reference**
(observable port behaviours, BOSS Katana SysEx, page layouts, expression
calibration). The Rust architecture is fresh — see
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the task graph.

## Quickstart

Prerequisites:

```powershell
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs       # for UF2 flashing (default runner)
# Optional, once you have a debug probe:
# cargo install probe-rs-tools
```

> ⚠️  The `rust-toolchain.toml` pins `channel = "stable"` and the
> `thumbv6m-none-eabi` target. On first `cargo build`, rustup will
> install both automatically.

Build:

```powershell
# from this directory
cargo build --bins --examples              # dev build (app binary + examples)
cargo build --bins --examples --release    # production build
```

The green gate (run before pushing) is both of:

```powershell
cargo build  --release --bins --examples
cargo clippy --release --lib --bins --examples -- -D warnings
```

Flash the app binary or a particular example — see below for the two paths.

> **Validating a change?** [`TESTING.md`](TESTING.md) is the full playbook —
> the gate, the on-hardware self-tests, on-device manual checks, the CDC /
> MIDI-monitor tooling, a per-feature validation matrix, and a pre-merge
> checklist.

## Flashing

### Today: UF2 over USB (no probe required)

This is the active default runner (`elf2uf2-rs -d` in
[`.cargo/config.toml`](.cargo/config.toml)).

1. Hold **Switch 1** on the MIDI Captain while connecting USB.
   The device enumerates as `RPI-RP2` mass storage.
2. From this directory, run:

   ```powershell
   cargo run --release --bin midicaptain     # the application firmware
   cargo run --release --example blink
   cargo run --release --example serial_echo
   cargo run --release --example midi_passthrough
   ```

   `elf2uf2-rs -d` converts the ELF to UF2, drops it onto the mounted
   drive, the bootloader flashes it and reboots into the new firmware.

If the MIDI Captain ever locks up in a way where Switch 1 + power-on
won't get you to BOOTSEL, the `serial_echo` example also honours the
1200-baud "touch" convention (TinyUSB / Arduino style). With that
firmware running, you can recover from a host shell:

```powershell
py ..\scripts\bootsel_hammer.py
```

The script opens the device's CDC port at 1200 baud and drops DTR; the
firmware sees this and calls `rom_data::reset_to_usb_boot(0, 0)`,
re-enumerating as `RPI-RP2`. Then drag a fresh UF2 onto it.

### When the probe arrives: `probe-rs run`

The Pi Debug Probe is on order. Once it's wired to the SWD pads on the
MIDI Captain's PCB (location: **TBD** — see
[`HARDWARE.md`](HARDWARE.md)):

1. Edit [`.cargo/config.toml`](.cargo/config.toml): comment the
   `elf2uf2-rs -d` line and uncomment the `probe-rs run --chip RP2040`
   line.
2. `cargo run --release --example blink` will flash + reset + stream RTT
   logs in one shot.

Alternative if you don't want to edit the file: override per-run:

```powershell
$env:CARGO_TARGET_THUMBV6M_NONE_EABI_RUNNER = "probe-rs run --chip RP2040"
cargo run --release --example blink
```

(Unset the env var, or open a fresh shell, to fall back to UF2.)

## Examples

Bring-up / transport sanity checks and per-module proof binaries. Keep
features *out* of these — they exercise one module each; real behaviour
lives under `src/`.

| Example | What it proves |
|---|---|
| `blink` | Chip + Embassy executor alive, PIO+DMA WS2812 driver works on GP7. Lights LED 0 of the chain through red → green → blue at ~3 Hz. |
| `serial_echo` | USB CDC ACM device on VID 0x2E8A (RP2040 standard), composite-friendly descriptors, echoes bytes, handles the 1200-baud BOOTSEL touch. |
| `midi_passthrough` | USB-MIDI device + DIN UART at 31250 baud, bidirectional bridge between them. Channel-voice messages only (no SysEx) — superseded by `midi_engine_test` for the real engine. |
| `display_splash` | Brings up the ST7789 (mipidsi 0.10, `Deg0`+offset(0,0), colour inversion ON) and renders the Remedy splash. **Hardware-validated.** |
| `display_widgets` | Animates `ValueBar` + `TextPanel`, logging dirty-flag gating (SPI quiet when nothing changed). |
| `encoder_test` | Logs `EncoderEvent`s: detented turns, velocity acceleration, debounced push. |
| `expression_test` | Logs `ExprEvent`s from both ADC pedals (GP27/GP28) with smoothing + calibration mapping. |
| `leds_test` | Cycles `LedFrame`s through the 30-pixel chain via the LED task (keeps every channel ≤ 32). |
| `storage_test` | Writes a distinctive value to every setting, reads it back through real flash, asserts the round-trip (and persistence across reboots). |
| `midi_engine_test` | Byte-exact codec self-test (Katana DT1/RQ1, Roland checksum, SysEx USB round-trip, running-status decode) vs the CircuitPython reference, then runs the live mux. |

## Logging

`defmt-rtt` carries log output. With UF2 flashing there is no host link
for logs — you'd need to add a USB-CDC logger task or wait for the
probe. With `probe-rs run`, logs stream automatically.

Default level is `info`. Override per-run:

```powershell
$env:DEFMT_LOG = "trace"
cargo run --release --example blink
```

## What's verified

- `cargo build --release --bins --examples` → clean (app binary + all examples).
- `cargo clippy --release --bins --examples -- -D warnings` → clean.
- **ST7789 display: hardware-validated** on real silicon (Pi Debug Probe):
  splash + widgets render upright, centred, flicker-free.
- MIDI codec: `midi_engine_test`'s self-test asserts byte-exactness against
  vectors from the CircuitPython reference (`remedy/lib/midi.py`).
- The other landed modules (encoder, expression, LEDs, storage) build clean
  and ship a proof example each; **on-hardware bring-up of those, and of the
  app binary's channel pipeline, is still pending** a probe session.

## What's next

**Wave 2 — integration (serial):** wire the landed HAL + MIDI modules into
the router in `src/bin/midicaptain.rs`, then build the config/page action
system (what each button does per page). See [`HANDOFF.md`](HANDOFF.md) for
the dependency map and the parallel/serial split, and
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the task graph.

## Repo conventions

- `Cargo.lock` is committed (this is a binary-producing crate).
- `target/` is gitignored locally.
- All firmware work belongs in this directory — `../remedy/` is the
  CircuitPython reference and stays read-only until the port catches up.

## Toolchain versions pinned

As of May 2026:

| Crate | Version | Note |
|---|---|---|
| `embassy-rp` | 0.10 | Latest stable; RP2040 HAL |
| `embassy-executor` | 0.10 | `platform-cortex-m` feature (was `arch-cortex-m`) |
| `embassy-usb` | 0.6 | Has both `cdc_acm` and `midi` classes |
| `embassy-time` | 0.5 | Tick driver via embassy-rp |
| `embassy-sync` | 0.8 | Bounded channels & mutexes |
| `embassy-usb` (driver) | 0.6 / 0.2 | USB-MIDI + CDC classes |
| `mipidsi` | 0.10 | ST7789 driver (hardware-validated) |
| `embedded-graphics` | 0.8 | Primitives + built-in mono fonts |
| `sequential-storage` | 7.2 | Flash KV settings store (`src/storage.rs`) |
| `smart-leds` | 0.4 | WS2812 colour type (PIO driver via embassy-rp) |
| `defmt` | 1.0 | Stable release |

`portable-atomic` is pulled in with the `critical-section` feature so
`static_cell` and a couple of embassy crates can use `AtomicBool::cas`
on Cortex-M0+ (which has no native CAS).
