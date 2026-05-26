# MIDICaptain Remedy — Rust + Embassy firmware

Rust port of the MIDI Captain firmware. **Status: bootstrap PoC.** Three
working example binaries exercise the WS2812 chain, the USB CDC port, and
both MIDI transports (USB-MIDI + DIN UART). The full app — pages,
display, settings, Katana SysEx — lands in follow-up sessions.

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
cargo build --examples              # dev build (small)
cargo build --examples --release    # production build
```

Flash a particular example — see below for the two paths.

## Flashing

### Today: UF2 over USB (no probe required)

This is the active default runner (`elf2uf2-rs -d` in
[`.cargo/config.toml`](.cargo/config.toml)).

1. Hold **Switch 1** on the MIDI Captain while connecting USB.
   The device enumerates as `RPI-RP2` mass storage.
2. From this directory, run:

   ```powershell
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

| Example | What it proves |
|---|---|
| `blink` | Chip + Embassy executor alive, PIO+DMA WS2812 driver works on GP7. Lights LED 0 of the chain through red → green → blue at ~3 Hz. |
| `serial_echo` | USB CDC ACM device on VID 0x2E8A (RP2040 standard), composite-friendly descriptors, echoes bytes, handles the 1200-baud BOOTSEL touch. |
| `midi_passthrough` | USB-MIDI device + DIN UART at 31250 baud, bidirectional bridge between them. Channel-voice messages only (no SysEx) in the PoC. |

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

- `cargo build --examples --release` → clean (3 ELFs produced).
- `cargo clippy --examples -- -D warnings` → clean.
- **Not** tested on hardware yet (no device available this session).
  Reports of successful flashing welcome.

## What's next

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the planned module roll-out.
Best next focus is the display driver, since it's the visible feedback
layer everyone notices first.

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
| `mipidsi` | (not yet) | ST7789 driver lands with display session |
| `sequential-storage` | (not yet) | Flash KV lands with config session |
| `defmt` | 1.0 | Stable release |

`portable-atomic` is pulled in with the `critical-section` feature so
`static_cell` and a couple of embassy crates can use `AtomicBool::cas`
on Cortex-M0+ (which has no native CAS).
