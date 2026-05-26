# Handoff — next session: display driver

You're picking up the Rust + Embassy port one session in. Read this top
to bottom before touching anything; it's calibrated to save you a
half-hour of rediscovery.

## TL;DR

- Branch: `rust-embassy-port` (off `SAFE_main`). Two commits land the
  bootstrap PoC. **Don't open or merge a PR against `main`** — the
  owner keeps this branch off main until the port can fully replace
  the CP firmware.
- `firmware/` builds clean (`cargo build --examples --release` and
  `cargo clippy --examples -- -D warnings`).
- Three example binaries work as compiled artifacts; **none have been
  flashed to hardware yet** (no device available last session). First
  thing you do on real hardware: flash `examples/blink` to confirm
  the WS2812 chain and PIO+DMA path. If that lights up LED 0 in
  red→green→blue, the chip and Embassy executor are healthy.
- Your job: stand up the ST7789 240×240 display.

## State of the codebase

```
firmware/
├── Cargo.toml                  embassy 0.10 / usb 0.6 / time 0.5 pinned
├── Cargo.lock                  ← committed (binary crate convention)
├── memory.x                    RP2040 boot2 + flash + RAM
├── build.rs                    stages memory.x, links cortex-m-rt / boot2 / defmt
├── rust-toolchain.toml         stable + thumbv6m-none-eabi
├── .cargo/config.toml          DUAL RUNNER (UF2 default; probe-rs commented)
├── .gitignore                  target/
├── src/
│   ├── lib.rs                  re-exports pins
│   └── pins.rs                 typed board map; USB VID 0x2E8A
├── examples/
│   ├── blink.rs                WS2812 PIO/DMA on GP7
│   ├── serial_echo.rs          USB CDC + 1200-baud BOOTSEL trigger
│   └── midi_passthrough.rs     USB-MIDI ↔ DIN UART bridge
├── README.md                   build/flash quickstart
├── ARCHITECTURE.md             task graph, channel design, crate rationale
├── HARDWARE.md                 pin map; "SWD pad location: TBD"
└── HANDOFF.md                  ← you are here
```

## Your task (priority order)

### 1. Add a display driver module (the main work)

Goal: render a static "MIDICaptain Remedy" splash screen on the ST7789,
upside-down, at full brightness. That's the floor; once you have a
splash, port the scene-graph patterns from
[`../remedy/lib/display.py`](../remedy/lib/display.py) into a
`src/display/` module.

Suggested layout (don't over-engineer — start with one file):

```
src/
├── display.rs          ← driver wrapper, public API: init/clear/render
└── examples/
    └── display_splash.rs   ← new example that uses it
```

Or if it grows organically: `src/display/{driver.rs, scene.rs,
palette.rs}` later. Start single-file.

#### Crate choices

| Crate | Version | Role |
|---|---|---|
| `mipidsi` | `0.10` (latest stable) | ST7789 driver. Has `Builder::st7789(...)` already. |
| `display-interface-spi` | `0.5` | SPI-byte protocol the mipidsi builder wants |
| `embedded-graphics` | `0.8` | Primitives (text, rect, lines) |
| `embedded-hal-bus` | `0.3` | For sharing the SPI bus if you ever need to. Probably overkill — start with `embedded_hal::spi::ExclusiveDevice` from `embedded-hal-bus`. |

mipidsi 0.10 changed its API significantly from 0.7/0.8 — don't trust
older blog posts or stale Stack Overflow answers. Use the docs at
<https://docs.rs/mipidsi/0.10> and the examples in
<https://github.com/almindor/mipidsi/tree/master/mipidsi/examples>.

For embedded-graphics text with the PCF fonts the CP firmware uses,
look at `embedded-graphics::mono_font` for built-ins or
`u8g2-fonts` if you need to keep the exact PTSans look. Bringing the
existing PCF files over is non-trivial — easiest path is to start with
`embedded_graphics::mono_font::ascii::FONT_10X20` or similar and chase
fidelity later.

#### Critical ST7789 init details (read these BEFORE writing code)

These come from
[`../remedy/lib/display.py:493-509`](../remedy/lib/display.py#L493):

- **`rowstart = 80`** (panel-specific offset). The ST7789's RAM is
  240×320; the 240×240 panel ships a 80-row offset in Y. mipidsi
  expresses this via `.display_offset(0, 80)` on the Builder.
  **Without this, the image will be offset / clipped.**
- **`rotation = 180`** because the display is mounted upside-down in
  the chassis. mipidsi: `.orientation(Orientation::new().rotate(Rotation::Deg180))`
  (check current API — may be `.flip_horizontal().flip_vertical()`
  depending on version).
- **`baudrate = 24_000_000`** (24 MHz). Both the CP code AND
  `firmware/src/pins.rs::DISPLAY_SPI_BAUD` agree. Don't go faster
  without scoping signal integrity.
- **No reset pin on the panel** — CP passes `reset=None`. The ST7789
  initializes from software-only reset commands. mipidsi handles this
  if you pass `NoPin` or omit the reset pin.

#### Pin assignments (from `src/pins.rs`, mirrored from
`remedy/lib/pins.py`)

| Signal | RP2040 pin | Notes |
|---|---|---|
| SPI1 SCK  | GP14 | hardware SPI1 |
| SPI1 MOSI | GP15 | display is write-only — no MISO |
| TFT CS    | GP13 | plain GPIO output |
| TFT DC    | GP12 | data/command select |
| Backlight | GP8  | PWM-capable. Plain `Output::High` is fine if you don't need dimming day one. |

#### Reference materials (in order of usefulness)

1. **[`../remedy/lib/display.py`](../remedy/lib/display.py)** —
   623 LOC. The CP-side display abstraction. Read top-to-bottom. The
   important classes:
   - `ColorPalette` (lines 35-128) — named colors, dim/dark variants
     computed on first use, conversion to displayio integers
   - `DisplayElement` (lines 130-176) — base class with dirty-flag
     rendering pattern. Port this concept to Rust as a trait.
   - `ValueBar` (178-313) — horizontal value bar with label,
     bg/fg/outline. The encoder feedback widget.
   - `TextPanel` (315-424) — labeled text box, multi-line.
   - `DisplayManager` (426-623) — owns the SPI bus and root group;
     creates layers; manages fonts. The init dance is in `_init_display`.
     Layered group structure (root → layers → elements).
2. **[`../remedy/lib/menu.py`](../remedy/lib/menu.py)** — 388 LOC.
   Settings menu UI: page navigation, encoder rotation = list nav,
   long-press to enter. Calibration wizard for expression pedals
   (3-step state machine). Worth a skim; ports later.
3. **OEM reference (compiled bytecode, opaque):**
   - `../MIDICAPTAIN_OEM_BACKUP/lib/adafruit_st7789.mpy` — the actual
     CP driver the OEM ships. Source is public:
     <https://github.com/adafruit/Adafruit_CircuitPython_ST7789>.
     Useful only to cross-reference init sequences if mipidsi gives
     you trouble. (TL;DR: it's just `adafruit_st7789.ST7789` wrapping
     `displayio` — the init magic lives in
     `adafruit_st7789/__init__.py` in that repo: a few `_INIT_SEQUENCE`
     bytes that mipidsi already encodes.)
   - `../MIDICAPTAIN_OEM_BACKUP/lib/midicaptain.mpy`,
     `midicaptain10s.mpy`, `midicaptain_ledon.mpy`, `midigeek.mpy`,
     `midigeek_C.mpy` — OEM-custom modules (compiled, no source).
     Boot-mode dispatched by `MIDICAPTAIN_OEM_BACKUP/code.py` lines
     336-356 based on key combo at power-on. These define the visible
     OEM behaviours (page layouts, footswitch maps, LED behaviour
     per mode). If you want to know exactly how the OEM presents the
     UI, you'd need to decompile with `mpy-cross` or
     <https://github.com/dhylands/mpy-cross-decompile>. Probably
     unnecessary — the `remedy/` CP firmware has already absorbed the
     useful patterns.
   - `../MIDICAPTAIN_OEM_BACKUP/fonts/` — PCF bitmap fonts the OEM
     uses (`PT40.pcf`, `PT60.pcf`, `PT75.pcf`, etc.). Bringing these
     over to Rust requires converting to a format `u8g2-fonts` or
     `embedded-graphics` understands. Punt on this; start with built-in
     monospace fonts.
4. **mipidsi examples in their repo** —
   <https://github.com/almindor/mipidsi/tree/master/mipidsi/examples>
   has examples for the embassy-rp + ST7789 combination. Start from
   one that uses `embassy_rp::spi::Spi` and `display_offset`.

### 2. (After display works) Update `HARDWARE.md` SWD pad location

The owner has reverse-engineering docs that aren't on this branch.
Ask them to share, then fill in
[`HARDWARE.md`](HARDWARE.md)'s `SWD debug pads` section. That unlocks
`probe-rs run` for live RTT logging the moment the Pi Debug Probe
arrives.

### 3. Add a real `src/bin/midicaptain.rs` application binary

The examples are PoC transport tests. Once you have a display driver
the application skeleton can start consolidating tasks per the graph
in [`ARCHITECTURE.md`](ARCHITECTURE.md). This may or may not be your
session's scope — at minimum, *don't* add display features into
`examples/blink.rs`; create a new example or the app binary instead.

## Quirks & gotchas you'll otherwise hit

These each cost ~15-30 min last session. They're all fixed in committed
code but if you touch the surrounding areas you'll re-encounter them.

| # | Symptom | Cause | Fix |
|---|---|---|---|
| 1 | `static_cell` fails to compile with "compare_exchange requires CAS" | Cortex-M0+ has no native CAS atomics | `portable-atomic = { version = "1", features = ["critical-section"] }` already in `Cargo.toml` |
| 2 | `feature 'arch-cortex-m' does not exist on embassy-executor 0.10` | Renamed in 0.10 release | Use `platform-cortex-m` (committed) |
| 3 | `feature 'defmt-03' does not exist on embedded-io-async` | Older name | Use `defmt` |
| 4 | `cargo:rustc-link-arg-bins=...` errors with "package does not have a bin target" | No `[[bin]]` in Cargo.toml; only examples | Use `cargo:rustc-link-arg=...` (applies to everything). `build.rs` already does this. |
| 5 | `expected SpawnToken, found Result<SpawnToken, SpawnError>` | embassy-executor 0.10 makes `#[task]`-attributed functions return `Result` from the macro's expanded callsite | `spawner.spawn(task_fn(args).unwrap())` — there's no `must_spawn` API in 0.10 |
| 6 | `LineCoding.data_rate is a private field` | `data_rate` became a method in embassy-usb 0.6 | Call as `.data_rate()` |
| 7 | `ControlChanged` has no `line_coding()` method in embassy-usb 0.6 | Method was added on `main` after 0.6.0; not yet released | Co-locate the BOOTSEL watcher with the CDC `Sender` (which DOES expose `line_coding()`). See `examples/serial_echo.rs::writer_and_watcher`. When a 0.6.x patch ships with the method, simplify by splitting watcher into its own task. |
| 8 | `PioWs2812` has 4 generic params (P, S, N, ORDER); turbofish gets noisy | embassy-rp 0.10 added ORDER param | Use a let-binding: `let mut ws: PioWs2812<'_, PIO0, 0, { pins::NEOPIXEL_COUNT }, _> = PioWs2812::new(...);` — leaves ORDER for inference |

## How to flash & test on hardware

### Without a probe (today's reality)

1. Hold **Switch 1** on the MIDI Captain while plugging in USB.
   Device enumerates as `RPI-RP2` mass storage.
2. From `firmware/`: `cargo run --release --example blink`
3. `elf2uf2-rs -d` converts the ELF → UF2 → drops onto the drive →
   device flashes and reboots.

If Switch 1 + power-on isn't getting you to BOOTSEL because the running
firmware is wedged, flash `serial_echo` first, then use the recovery
channel:

```powershell
py ..\scripts\bootsel_hammer.py
```

(opens the device's CDC port at 1200 baud, drops DTR; firmware detects
this and calls `rom_data::reset_to_usb_boot(0, 0)`)

### With a probe (once one arrives)

Edit `.cargo/config.toml`: comment the `elf2uf2-rs -d` line, uncomment
`probe-rs run --chip RP2040`. Wire to SWD pads
(location TBD — see `HARDWARE.md`). Then `cargo run` flashes AND
streams RTT logs.

## Branch / repo invariants

- Don't modify `../remedy/`, `../webapp/`, `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, or root `../CLAUDE.md`. All firmware work goes in
  `firmware/`.
- Don't expose USB MSC anywhere. The whole point is the device owns
  flash exclusively.
- Don't merge to `main`. The branch lives on `rust-embassy-port`
  (forked from `SAFE_main`) until the port fully replaces CP.
- Commit `Cargo.lock` (it's a binary-producing crate).
- Run `cargo clippy --examples -- -D warnings` before pushing.

## Watch-outs specific to display work

- **CP boot.py SPI claim bug**: the CP firmware has an entire NVM
  guard mechanism (`remedy/lib/display.py:_nvm_guarded_reset`) because
  `import storage` in boot.py claims SPI1 pins as a side effect on
  CP10. **This does not apply to Rust** — we have no `storage` module
  doing that. You don't need to port any of the
  `_nvm_*` / `release_displays()` complexity. mention this in the
  display module's header comment so it doesn't get re-introduced.
- **PCF fonts**: `MIDICAPTAIN_OEM_BACKUP/fonts/*.pcf` (PT40, PT60,
  PT75 = Adafruit PT-Sans variants). They're bitmap fonts in X11 PCF
  format. Rust embedded-graphics doesn't read PCF natively. Options:
  (a) start with `embedded_graphics::mono_font` built-ins (ugly but
  works), (b) convert PCF → BDF → use `u8g2-fonts`, (c) bake a custom
  font compiler. Don't bikeshed this in session two — pick (a).
- **Splash content**: the OEM splash is in `MIDICAPTAIN_OEM_BACKUP/
  logo.bmp` and `wallpaper/`. Don't reuse them — the "Remedy" branding
  is intentionally distinct.

## Concrete deliverable for next session

When you're done, the user should be able to:

```powershell
cargo run --release --example display_splash
```

…and see "MIDICaptain Remedy" rendered legibly on the ST7789 (right-
side-up from the user's POV — i.e., display rotation applied). Plus
clean clippy, plus the display module documented in
`ARCHITECTURE.md`'s "Project layout" section (just move it from
"Future modules" to actual).

Estimated effort: 2-4 hours including hardware iteration. Worst case
is the rotation/offset combo; the cheat code is `rowstart=80` +
180° rotation + verify by drawing a single coloured pixel at (0, 0)
and (239, 239) before drawing text.

Good luck.
