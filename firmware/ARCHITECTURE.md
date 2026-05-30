# Architecture — Rust + Embassy firmware

The CircuitPython implementation in [`../remedy/`](../remedy/) is built
around one giant polling loop in `main.py`: read every input, dispatch
events, draw the display, sleep, repeat. That's idiomatic for CP and it
works for a 10-switch foot controller, but it bakes worst-case latency
into every subsystem. A slow display redraw delays button detection;
expression-pedal smoothing burns CPU even when nothing is touching the
pedal.

The Rust port discards that loop entirely. Embassy gives us cooperative
async tasks driven by hardware interrupts, plus typed compile-time
ownership of every peripheral. So:

- Each subsystem runs as its own task, woken only when something
  actually happens (button edge, UART RX, USB packet, encoder click).
- Tasks communicate through `embassy_sync::channel::Channel<…, RAW, N>`
  — bounded MPSC queues with explicit back-pressure semantics.
- No shared mutable state. The few things that ARE shared (e.g. current
  page index, settings) live in single-owner state machines that the
  rest of the system reads through query messages.

This file is the contract for future sessions. If you're adding a
subsystem, place it on the task graph below and document its channel(s).

## Project layout

```
firmware/
├── Cargo.toml
├── memory.x                ← RP2040 flash + RAM layout
├── build.rs                ← stages memory.x, links cortex-m-rt + boot2
├── rust-toolchain.toml     ← pins stable + thumbv6m-none-eabi
├── .cargo/config.toml      ← dual runner (UF2 default, probe-rs alt)
├── src/
│   ├── lib.rs              ← `pub mod pins, events, display, ui, hal, midi, storage`
│   ├── pins.rs             ← board pin map (GPIO numbers, NeoPixel chain order, USB IDs)
│   ├── events.rs           ← frozen channel contracts (Button/Encoder/Expr/MidiRx/MidiCmd/LedFrame/DisplayCmd)
│   ├── display.rs          ← ST7789 driver wrapper (mipidsi 0.10 + embedded-graphics)
│   ├── ui/                 ← dirty-flag scene graph atop display.rs
│   │   ├── mod.rs          ← Widget trait, Palette/Color re-exports
│   │   ├── palette.rs      ← eager const Rgb565 palette (dim/dark = const fn)
│   │   ├── element.rs      ← Widget trait (render → bool, mark_dirty)
│   │   ├── value_bar.rs    ← 0..127 horizontal bar widget (delta-paint, no flicker)
│   │   └── text_panel.rs   ← bordered multi-line text widget (heapless::String)
│   ├── hal/                ← HAL tasks: each owns a peripheral, emits events
│   │   ├── mod.rs          ← module re-exports (buttons still inline in bin, see below)
│   │   ├── encoder.rs      ← IRQ-driven quadrature + accel + debounced push → EncoderEvent
│   │   ├── expression.rs   ← async-ADC pedals (GP27/28) + calibration → ExprEvent
│   │   └── leds.rs         ← LedFrame → 30-px WS2812 chain on GP7 (PIO0+DMA)
│   ├── midi/               ← MIDI engine (single owner of USB-MIDI + DIN UART0)
│   │   ├── mod.rs          ← re-exports
│   │   ├── mux.rs          ← USB+DIN merge → MidiRx; MidiCmd → both transports
│   │   ├── sysex.rs        ← streaming SysEx (de)framing across USB-MIDI CINs
│   │   └── katana.rs       ← Roland checksum + DT1/RQ1 + Katana builders (port of remedy/lib/midi.py)
│   ├── storage.rs          ← flash KV settings store (sequential-storage over embassy_rp::flash)
│   └── bin/
│       └── midicaptain.rs  ← application binary: buttons → router → display slice
├── examples/               ← runnable transport / bring-up + per-module proof tests
│   ├── blink.rs
│   ├── serial_echo.rs
│   ├── midi_passthrough.rs
│   ├── display_splash.rs   ← bring up ST7789, render Remedy splash
│   ├── display_widgets.rs  ← animate ValueBar + TextPanel, log dirty-flag gating
│   ├── encoder_test.rs     ← log EncoderEvents (turns + accel + push)
│   ├── expression_test.rs  ← log ExprEvents from both pedals
│   ├── leds_test.rs        ← cycle LedFrames (channels ≤ 32)
│   ├── storage_test.rs     ← write/read-back every setting through real flash
│   └── midi_engine_test.rs ← byte-exact codec self-test vs CP reference + live mux
├── README.md               ← build/flash quickstart
├── ARCHITECTURE.md         ← this file
└── HARDWARE.md             ← pin map, SWD pads (VERIFIED), geometry/colour notes
```

The ST7789 path is **hardware-validated** (geometry `Deg0`+offset(0,0),
colour inversion ON, SWD flashing via Pi Debug Probe). All Wave-1 modules
above (`events`, `hal/*`, `midi/*`, `storage`) have **landed and pass the
green gate**, each with a proof example — but they are **not yet wired into
the router**: `bin/midicaptain.rs` still runs only the
buttons→router→display skeleton. Connecting them is Wave 2 (see below). The
application binary remains the live integration point — every subsystem
joins it as another task feeding the router.

Still to land:

```
src/
├── hal/buttons.rs          ← lift the inline footswitch debouncer out of bin/ (→ ButtonEvent)
├── config/                 ← per-button/per-page action table (CC/PC/SysEx/page-nav); serde-toml from flash KV
├── sync/                   ← COBS+CRC16 wire protocol for webapp sync (USB CDC)
└── app.rs                  ← extract the router/state machine out of bin/ once it grows
```

Today the buttons/router/display tasks live inline in
`bin/midicaptain.rs`. As Wave-2 integration grows the router, lift the
inline footswitch task into `src/hal/buttons.rs` and the router into
`src/app.rs`; the bin becomes thin wiring. (Storage deliberately shipped as
a direct async accessor, not a task — see the task-graph note below.)

## Task graph (target)

```
                  ┌──────────────┐
   GPIO IRQs ────▶│ buttons task │──ButtonEvent──┐
                  └──────────────┘               │
                  ┌──────────────┐               │
   GPIO IRQs ────▶│ encoder task │──EncEvent─────┤
                  └──────────────┘               │
                  ┌──────────────┐               │
   ADC DMA ──────▶│ expr.   task │──ExprEvent────┤
                  └──────────────┘               │
                                                 ▼
                                         ┌───────────────┐
                                         │ event router  │
                                         │  (app state)  │
                                         └───┬───┬───┬───┘
                                MidiCmd  ◀───┘   │   └───▶ LedFrame
                                                 │            │
                                            DisplayCmd        │
                                                 ▼            ▼
                                         ┌───────────────┐  ┌───────────┐
                                         │ display task  │  │ leds task │
                                         │   (30 fps)    │  │ (WS2812)  │
                                         └───────────────┘  └───────────┘
                                                                          
   USB IRQ ──────▶┌──────────────┐                 ┌─────────────────┐
                  │ usb device   │◀────MidiCmd─────│ midi mux task   │──▶ UART0 TX
   USB MIDI ─────▶│   task       │──UsbMidiRx──────│                 │◀── UART0 RX
   USB CDC  ─────▶│              │──CdcRx──────────│                 │
                  └──────────────┘                 └─────────────────┘
                                                            │
                                                       (router input)
```

Concretely:

| Task | Wakes on | Sends | Receives |
|---|---|---|---|
| `buttons` | GPIO IRQ (all 10 footswitches + encoder push) | `ButtonEvent` → router | — |
| `encoder` | GPIO IRQ on quadrature edges | `EncoderEvent` → router | — |
| `expression` | ADC DMA done (period polled @ ~100 Hz) | `ExprEvent` → router | — |
| `usb_device` | USB CTRL IRQ | raw USB-MIDI / CDC bytes → mux | `MidiCmd`, `CdcResp` |
| `midi_mux` | UART RX IRQ or channel msg | router input | `MidiCmd` from router |
| `router` | Any input channel msg | `MidiCmd`, `DisplayCmd`, `LedFrame` | All event channels |
| `display` | `DisplayCmd` or 30 Hz ticker | SPI frames to ST7789 | `DisplayCmd` |
| `leds` | `LedFrame` | WS2812 DMA writes | `LedFrame` |

Channel capacities are 8–16 in the PoC; tune per real-world load.

**Storage is not a task.** `src/storage.rs` shipped as a plain async
accessor (`Storage::load`/`store`), *not* a channel-driven task — the
settings store is touched only at infrequent boot-load / menu-save points,
so a dedicated task + `StorageReq` channel would be ceremony with no
back-pressure to manage. Callers (the boot path, the settings menu, the
expression-calibration save) `await` it directly. It owns the `FLASH`
peripheral; its blocking→async shim keeps it off the DMA subsystem
entirely. (Revisit only if a future caller needs concurrent access while a
multi-sector erase is in flight.)

## Channel design rules

1. **Bounded, never unbounded.** Out-of-memory is a worse failure than
   dropping one MIDI event. All channels have a compile-time depth.
2. **Drop-newest on overflow** for time-sensitive events (MIDI, LED
   frames). Use `try_send` and ignore the error. The next event
   overwrites the lost one's intent anyway.
3. **Block-the-producer for state changes** (config writes, page
   navigation). Use `send().await`; the queue depth is set so this
   never deadlocks under realistic load.
4. **One owner per peripheral.** A task that holds `UART0` is the only
   thing that can talk to UART0. To send DIN MIDI from elsewhere, send
   a `MidiCmd` to the mux task. This eliminates locking entirely.

## Why these crate choices

The PoC pulls in the minimum to prove the transports. Where alternatives
exist, here's why these:

- **embassy-rp** vs. `rp-hal`: embassy-rp is the only one with first-
  class async DMA + interrupt-driven USB + interrupt-driven UART out of
  the box. `rp-hal` is fine for sync code; we want async.
- **embassy-usb** vs. `usb-device`: embassy-usb is the only async USB
  stack that fits cleanly with embassy-executor. Has both `cdc_acm` and
  `midi` classes already; we don't reimplement either.
- **smart-leds** (used implicitly via PIO WS2812 driver): standard color
  type. The PIO program ships in embassy-rp under
  `pio_programs::ws2812`.
- **mipidsi 0.10** for the ST7789 (now wired up — see `src/display.rs`):
  widely adopted, works with `embedded-graphics`, supports the 180°
  rotation and 80-row offset we need. Ships its own
  `interface::SpiInterface` — no separate `display-interface-spi`
  crate (that was the 0.7/0.8 pattern).
- **embedded-hal-bus** for `ExclusiveDevice`: wraps embassy-rp's
  blocking `Spi` (an `SpiBus`) into the `SpiDevice` that mipidsi's
  interface wants.
- **embedded-graphics 0.8** for primitives + built-in mono fonts. PCF
  font parity with the OEM PTSans set is deliberately out of scope for
  v1 — using `mono_font::ascii::FONT_10X20` until the UI layer
  stabilises and font fidelity becomes worth the effort.
- **sequential-storage** for flash KV (landed — see `src/storage.rs`):
  purpose-built for flash wear-leveling. Replaces the CP NVM hack for
  expression pedal calibration; the same store holds all settings (MIDI
  channel, brightnesses, both pedal calibrations). A 64 KB region at the
  top of flash, kept disjoint from the firmware image by `memory.x`.

Notably absent: **no USB MSC**. This is intentional. The whole point of
the rewrite is that the device owns its flash exclusively — no host /
device write races that could corrupt the filesystem. The webapp sync
protocol (next-but-one session) rides on USB CDC instead.

## Decisions that may need revisiting

- **Application binary has landed.** `src/bin/midicaptain.rs` exists
  (buttons→router→display skeleton); the examples are now transport /
  bring-up tests. CI-equivalent check is
  `cargo build --release --bins --examples` and
  `cargo clippy --release --bins --examples -- -D warnings` (note
  `--bins`, added when the binary landed).
- **All-defmt logging, no panic redirect.** If we never get a probe,
  defmt-rtt is just bytes shouted into the void during UF2 boot. The
  USB CDC logger task is a reasonable backup; bring it in only when
  needed because it inflates the binary by ~40 KB.
- **`portable-atomic` with critical-section.** Required by
  `static_cell` on Cortex-M0+. If we move to RP2350 (Cortex-M33) later,
  this dependency becomes superfluous.
- **USB PID is 0x102D**, picked unilaterally. If we ever ship publicly,
  apply for a real PID (or use a sub-VID partner range).
- **Channel capacities (8 / 16).** Educated guess. Instrument with
  `defmt` once on hardware and adjust.
- **SysEx fragmentation — handled (landed).** The `midi_passthrough` PoC
  was channel-voice only; the real engine (`src/midi/{mux,sysex}.rs`) now
  reassembles SysEx across USB-MIDI 4-byte packets (CIN 0x4/0x5/0x6/0x7)
  and the DIN byte stream, and packetises outbound SysEx byte-exact with
  the DIN stream. `midi_engine_test.rs` proves the round-trip.

## Read this if you're starting the next session

1. `firmware/src/pins.rs` is the single source of truth for the board
   layout. If a peripheral comes up missing, check there first.
2. `examples/` is for transport sanity-checks. Don't pile features into
   them — add a new module under `src/` instead.
3. The CP firmware in `../remedy/lib/` is the behavioural reference for
   anything user-visible (LED brightness curves, encoder acceleration,
   tuner pitch detection, Katana SysEx addresses). Read it first, port
   the behaviour, do not copy the polling architecture.
4. `cargo build --examples --release` and
   `cargo clippy --examples -- -D warnings` should always pass. If
   they don't, fix that before pushing.
