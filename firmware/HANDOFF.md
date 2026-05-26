# Handoff — next session: hardware validation + UI scene graph

You're picking up the Rust + Embassy port two sessions in. The bootstrap
scaffold and the ST7789 display driver both compile clean; neither has
been flashed yet. Read this top-to-bottom before touching anything.

## TL;DR

- Branch: `claude/stupefied-euclid-34d8b5` (or whatever GitHub renamed
  it to after merge), stacked on `rust-embassy-port` which is the
  PR #2 branch. **Don't open or merge a PR against `main`** — the
  port stays off `main` until it can fully replace the CP firmware.
  See your memory file `project-rust-port-branching` for the durable
  rule.
- `firmware/` builds clean
  (`cargo build --examples --release` and
  `cargo clippy --examples --release -- -D warnings`).
- Four example binaries compile; **none have been flashed yet**. Your
  first move on real hardware is `cargo run --release --example
  display_splash`. If you see two pixels in opposite corners with the
  chassis label upright, plus centred "MIDICaptain Remedy" text,
  geometry is correct and you're cleared for the UI work.
- Your real task: stand up the UI scene-graph layer
  (`DisplayElement` / `ValueBar` / `TextPanel` port from
  `remedy/lib/display.py`).

## What landed last session

```
firmware/
├── src/
│   ├── display.rs           NEW — mipidsi 0.10 wrapper, type aliases,
│   │                              init() factory, 80-row offset + 180°
│   │                              rotation baked in. No reset pin. No
│   │                              NVM-guarded-reset (CP-only quirk).
│   ├── lib.rs               +  pub mod display;
│   └── pins.rs              unchanged
├── examples/
│   └── display_splash.rs    NEW — corner pixels at (0,0) red and
│                                  (239,239) green + centred text.
├── Cargo.toml               + mipidsi 0.10, embedded-graphics 0.8,
│                              embedded-hal 1.0, embedded-hal-bus 0.3.
│                              NOT display-interface-spi — mipidsi 0.10
│                              ships its own interface::SpiInterface.
└── ARCHITECTURE.md          display moved from "Future modules" to
                             actual; new ui/ slot for scene-graph layer.
```

The PR for this work is **#3 (stacked on #2)**. When #2 merges to
`SAFE_main`, GitHub auto-rebases #3's base.

## Your task (priority order)

### 1. Flash `display_splash` and validate geometry (15 min if it works)

```powershell
# From firmware/:
cargo run --release --example display_splash
```

Hold Switch 1 at power-on to enter BOOTSEL (or run
`py ..\scripts\bootsel_hammer.py` against `serial_echo` if you have
that flashed). `elf2uf2-rs -d` will drop the UF2.

**Expected (success):** Dark grey screen with a thin border, "MIDICaptain
Remedy" centred in white, "Rust + Embassy port" subtitle below. Single
red pixel at top-left, single green pixel at bottom-right — *from the
viewer's POV with the chassis upright*.

**Possible failures and the fix:**

| Symptom | Likely cause | Fix in `src/display.rs` |
|---|---|---|
| Blank screen, backlight on | DC pin wrong, or SPI silent | Check GP12 (DC) and GP14/15 (SPI1 SCK/MOSI) against `pins.rs` |
| Mirrored text (reads R→L) | Orientation off | Toggle `.flip_horizontal()` on the `Orientation` |
| Image scrolled vertically | Wrong `display_offset` | Y offset must be 80, not 0 or 40 |
| Text upside-down | Missing rotation | Confirm `Rotation::Deg180`, not `Deg0` |
| Garbled colours (red↔blue) | Colour order | `.color_order(ColorOrder::Bgr)` |
| Corner pixels visible at wrong coords | Offset + rotation interaction | Read [`../remedy/lib/display.py`](../remedy/lib/display.py) — CP's working config is the ground truth |

The corner-pixel pattern is your cheat code. Drawing both pixels
*before* any rotation/offset experimentation makes the geometry bug
visible in one frame.

### 2. UI scene-graph layer (the real session work)

Port the three dirty-flag widgets from
[`../remedy/lib/display.py`](../remedy/lib/display.py) to Rust. The CP
code is 623 LOC of well-organised class hierarchy. Take it one widget
at a time:

```
firmware/src/
├── display.rs               ← driver (do not bloat — keep as facade)
└── ui/                      ← NEW — scene graph
    ├── mod.rs               ← pub use, common types
    ├── palette.rs           ← ColorPalette: named colours, dim/dark
    │                          variants. Port the COLORS dict at
    │                          remedy/lib/display.py:44-67.
    ├── element.rs           ← Drawable trait + dirty flag.
    │                          remedy/lib/display.py:130-176.
    ├── value_bar.rs         ← Encoder feedback widget.
    │                          remedy/lib/display.py:178-313.
    └── text_panel.rs        ← Labeled text box.
                               remedy/lib/display.py:315-424.
```

#### The dirty-flag pattern in Rust

CP uses an OO base class with `mark_dirty()` / `clear_dirty()`. Rust's
analogue is a small trait:

```rust
pub trait Widget {
    /// Draw the widget into the target if anything has changed since
    /// the last call. Returns true if drawing actually happened.
    fn render<D: DrawTarget<Color = Rgb565>>(
        &mut self,
        target: &mut D,
    ) -> Result<bool, D::Error>;

    /// Force a redraw on the next render() call.
    fn mark_dirty(&mut self);
}
```

The key insight from CP: dirty-flag tracking is *per-widget state*, not
per-frame. A `ValueBar` owns its last-rendered value and only redraws
when the value changes. Don't try to be clever with a global dirty
region — embedded-graphics has no scissor concept, and partial redraws
on SPI are bandwidth-limited anyway. Just gate the whole-widget redraw.

#### Colour-palette design

CP's `ColorPalette` lazily caches dim/dark variants. In Rust, do the
opposite: compute *eagerly* at compile time. Named colours are `const`
Rgb565s, and `dim()` / `dark()` are `const fn`s that return new Rgb565s
by bit-shifting. No cache needed — the compiler memoises constants for
free.

```rust
pub const RED: Rgb565 = Rgb565::new(31, 0, 0);
pub const fn dim(c: Rgb565, factor: u8) -> Rgb565 { /* div */ }
```

(`embedded-graphics` `Rgb565` is 5/6/5-bit per channel; the CP code's
255-scale RGB tuples will need scaling down.)

#### Demo binary

Add `examples/display_widgets.rs` that animates a `ValueBar` and
updates a `TextPanel` — proves the dirty-flag gating actually reduces
draw calls. Use `defmt::info!` to log "redrew bar" / "skipped bar" per
frame.

### 3. Application binary (`src/bin/midicaptain.rs`) — only if you have time

This is where the task graph in
[`ARCHITECTURE.md`](ARCHITECTURE.md) starts to materialise. First task
to wire up: `buttons` → `router` → `display`. Stub the router as
"forward button events to a `TextPanel`". This proves the channel
plumbing works.

If you don't get here, leave it for session four. Don't half-do it.

## Quirks & gotchas

Add new ones as you hit them. The bootstrap session's list is
preserved at the bottom; here are the *display-driver* gotchas worth
inheriting:

| # | Symptom | Cause | Fix |
|---|---|---|---|
| D1 | `Output::new` / `Spi::new_blocking_txonly` reject pin args | embassy-rp 0.10 wraps pins in `Peri<'d, T>` | Struct fields holding pins must be typed `Peri<'static, PIN_xx>`, not bare `PIN_xx`. See `DisplayPeripherals` in `display.rs`. |
| D2 | `mipidsi::Display` generic type is unwieldy in user code | RST=`NoResetPin`, DI is a 3-param SpiInterface | Use the `RemedyDisplay` type alias from `display.rs`. Don't redefine. |
| D3 | `display-interface-spi` 0.5 doesn't exist on the dep tree | mipidsi 0.10 ships its own `SpiInterface` | Use `mipidsi::interface::SpiInterface`. HANDOFF was stale on this. |
| D4 | `#[cfg_attr(feature = "defmt", derive(defmt::Format))]` warning | We don't define a `defmt` feature on our crate (defmt is always-on) | Derive `defmt::Format` unconditionally. |
| D5 | Two `defmt` versions in the dep graph (1.0 + 0.3) | embedded-graphics-core pulls 0.3 transitively | Harmless — coexist as separate graph nodes. Costs a few KB binary, no functional issue. Leave it. |

Bootstrap-session gotchas (#1–#8 in the previous HANDOFF) are still
valid; check `git log -p firmware/HANDOFF.md` if you need them.

## How to flash & test on hardware

Same as before — `cargo run --release --example <name>` produces a UF2
via `elf2uf2-rs -d`. Recovery via `py ..\scripts\bootsel_hammer.py`
against a flashed `serial_echo` works.

The SWD pad location in [`HARDWARE.md`](HARDWARE.md) is still TBD. If
the owner shares their reverse-engineering docs this session, fill it
in. That unlocks `probe-rs run` for live RTT logging — much faster
iteration than UF2 reflash.

## Branch / repo invariants

(Re-stated from previous HANDOFF — these don't change.)

- Don't modify `../remedy/`, `../webapp/`, `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, or root `../CLAUDE.md`. All firmware work goes in
  `firmware/`.
- Don't expose USB MSC. The whole point is the device owns flash
  exclusively.
- Don't merge to `main`. Stack on `rust-embassy-port` (or push directly
  if PR #2 has merged by the time you read this).
- Commit `Cargo.lock` (binary-producing crate).
- Run `cargo clippy --examples --release -- -D warnings` before
  pushing.

## Watch-outs specific to UI work

- **Memory.** Each widget owns a small amount of state (last value,
  string buffer). Use `heapless::String<N>` for text, not `alloc`. We
  have no allocator and don't want one.
- **Font fidelity vs. effort.** The OEM uses PTSans PCF bitmap fonts.
  embedded-graphics doesn't read PCF natively. *Do not* bikeshed font
  conversion this session. Built-in `FONT_10X20` is ugly but works;
  ship the scene graph first, swap fonts later. If/when you do it,
  `u8g2-fonts` reads BDF (convert PCF→BDF with `pcf2bdf`).
- **Don't port `DisplayManager`'s layer/mode-switching directly.** CP
  needs it because `displayio.Group` is its API. Rust has no implicit
  scene tree — `render()` calls are just sequential `Drawable.draw()`
  invocations against the target. A "mode" is just "which widgets does
  the app render this frame." Encode it in the app state machine, not
  the display layer.
- **Tuner mode is its own beast.** `remedy/lib/tuner.py` drives a
  large note glyph + cents-deviation needle. Skip it this session
  unless the scene graph lands fast.

## Concrete deliverable for next session

When you're done:

```powershell
cargo run --release --example display_widgets
```

…shows an animating value bar (use a `Ticker` to cycle 0..127) and a
text panel showing the current value, with `defmt::info!` showing that
the bar redraws only when the value changes and the panel redraws only
when the displayed string changes. Plus clean clippy, plus an updated
ARCHITECTURE.md and a (smaller) HANDOFF.md.

Estimated effort: 3–5 hours. The hard part is choosing how granular
the trait surface is (one `Widget` trait? `Drawable` + `DirtyTracker`
split?). Don't overthink — one trait, lift only what hurts. The right
abstractions reveal themselves once two or three widgets are in.

## On context handoffs (meta)

The previous HANDOFF was load-bearing because the task was
"implement a non-trivial driver from scratch with subtle hardware
quirks." This one can be shorter because:

- The driver is in the repo — read `src/display.rs`, not prose about it.
- The scene-graph port has a clear reference (`remedy/lib/display.py`).
- The gotchas list is shrinking as the codebase grows past its
  bootstrap.

If you're handing off again, follow the same principle: encode
*forward-looking* knowledge (next task, scope guardrails, known
gotchas not yet hit) in HANDOFF.md, and let code + commit history +
PR description carry the *backward-looking* knowledge (what was done
and why).

Good luck.
