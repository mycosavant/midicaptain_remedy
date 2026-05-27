# Handoff — next session: hardware validation + application binary

Three sessions in. Bootstrap, ST7789 driver, and dirty-flag scene-graph
are all compiled and committed; nothing has been flashed yet. Read this
top-to-bottom before touching anything.

## TL;DR

- Branch: `claude/stupefied-euclid-34d8b5` → `claude/optimistic-davinci-85355e`,
  stacked on `rust-embassy-port` (PR #2). **Don't open a PR against
  `main`** — see your memory `project-rust-port-branching`.
- `firmware/` builds clean (`cargo build --examples --release` and
  `cargo clippy --examples --release -- -D warnings`).
- Five example binaries compile. `display_widgets` is the live demo of
  the scene-graph layer landed last session.
- Hardware validation is still **outstanding**: nothing in
  `examples/display_*.rs` has actually been flashed yet. First move on
  real hardware: `cargo run --release --example display_splash`. If
  geometry checks out (corner-pixel cheat code), follow up with
  `display_widgets` and watch the `defmt` log for dirty-flag gating.

## What landed last session

```
firmware/
├── src/
│   ├── ui/                       NEW — scene-graph layer
│   │   ├── mod.rs                Widget trait + Palette re-exports
│   │   ├── palette.rs            const Color (8-bit) + const fn
│   │   │                         dim/dark + const to_rgb565
│   │   ├── element.rs            Widget trait: render → bool, mark_dirty
│   │   ├── value_bar.rs          0..=127 horizontal bar, outline + fill
│   │   └── text_panel.rs         bordered multi-line text, heapless::String
│   ├── lib.rs                  + pub mod ui;
│   └── display.rs              unchanged (still the mipidsi facade)
├── examples/
│   └── display_widgets.rs        NEW — animates ValueBar 0..127..0 at
│                                 20 Hz, updates TextPanel on band edges,
│                                 logs every frame's draw/skip decision
├── Cargo.toml                  + heapless 0.9 (already transitive)
└── ARCHITECTURE.md             ui/ moved from "Future modules" to actual
```

PR for this work: **#5 (stacked on #4 stacked on #2)**. When #2 + #4
merge to `rust-embassy-port`, GitHub auto-rebases.

## Your task (priority order)

### 1. Flash and validate (the long-deferred step)

Two example binaries to drive:

```powershell
# From firmware/:
cargo run --release --example display_splash    # static splash + corner pixels
cargo run --release --example display_widgets   # animated, with defmt log
```

Enter BOOTSEL: hold Switch 1 at power-on, *or* run
`py ..\scripts\bootsel_hammer.py` against a running `serial_echo`.

**`display_splash` success criteria** (from session 2's HANDOFF — still
the source of truth for geometry):

- Dark grey screen, thin border, "MIDICaptain Remedy" centred white,
  "Rust + Embassy port" subtitle below.
- Single red pixel at the *viewer's* top-left.
- Single green pixel at the *viewer's* bottom-right.
- See `git show f000029` for the symptom→fix table if anything's off.

**`display_widgets` success criteria** (new):

- Cyan value bar grows/shrinks across the upper half of the screen on a
  ~4-second triangle wave.
- Blue text panel below flips through "LOW" / "MID" / "HIGH" at value
  bands 0–41, 42–83, 84–127.
- The RTT log should show `bar=DREW` every frame and `panel=DREW` only
  on the ~4 frames per sweep where the band crosses. If the panel says
  `DREW` *every* frame, dirty-flag gating is broken — likely a setter
  that always marks dirty regardless of equality.

If the bar's value text overlaps the fill awkwardly, that's a font-
metrics issue not a correctness bug. Document it; don't chase fonts
this session (see "Font fidelity" below).

### 2. Application binary — `src/bin/midicaptain.rs`

The examples are transport tests. Time for the first slice of the real
app. Scope this *small*: implement the minimum task graph from
[`ARCHITECTURE.md`](ARCHITECTURE.md) needed to prove the channel pump:

```
buttons task ──ButtonEvent──▶ router task ──DisplayCmd──▶ display task
```

Concretely:

- One Embassy task per box. Bounded `embassy_sync::channel::Channel`
  between them (capacity 8 is fine for the demo).
- `buttons` task: poll 1–2 GPIO inputs with debouncing (port the simple
  pattern from `../remedy/lib/hardware.py::ButtonHandler` — 5 ms
  settle is plenty). Channel sends `ButtonEvent { id, pressed }`.
- `router` task: maintain a counter per button, send `DisplayCmd::ShowCounter(id, n)`
  whenever a press fires.
- `display` task: owns `RemedyDisplay` + one `TextPanel` per button.
  Renders on `DisplayCmd` receipt. No 30-Hz tick yet — pure event-
  driven.

This is the smallest possible end-to-end vertical slice. Once it
works, the rest of the task graph fills in by analogy.

**Don't** wire up the encoder, LEDs, MIDI, or expression pedals yet —
each deserves its own session-sized chunk. The point here is to prove
the channel plumbing.

### 3. Stretch — encoder + WS2812 LED feedback

If the app binary lands quickly: add the encoder task (quadrature
decode from `remedy/lib/hardware.py::EncoderHandler`) and pipe its
turns into the same router. Optionally extend `router` to drive a tiny
WS2812 strip frame ("LED for button N lights when pressed"). The PIO
driver pattern is in `examples/blink.rs`.

Don't half-finish this. If you can't get a full vertical slice working,
leave it for session five.

## Quirks & gotchas — additions

Session 3 was uneventful; no new gotchas worth inheriting beyond:

| # | Symptom | Cause | Fix |
|---|---|---|---|
| U1 | `defmt::info!` rejects `{:>4}` width specifiers | defmt format strings are a tiny subset of Rust's; only positional and `:?` work | Drop the width spec or use Rust's `core::fmt::Write` against a `heapless::String` buffer first |

Previous gotchas (D1–D5 display, #1–#8 bootstrap) remain in
`git log -p firmware/HANDOFF.md` if you need them.

## Watch-outs specific to the app binary

- **Single executor.** The default thread-mode executor is fine. Don't
  reach for the interrupt executor unless something measurable demands
  it.
- **`#[embassy_executor::main]` returns one task.** Spawn the rest with
  `spawner.spawn(task_fn(args).unwrap())`. Gotcha #5 from the bootstrap
  HANDOFF: there's no `must_spawn` API in 0.10.
- **Channels need a `'static` ground.** Use `static_cell::StaticCell`
  to land them in `.bss` (look at how `display.rs`'s `SPI_BUF` does
  it). Or `static CHANNEL: Channel<...> = Channel::new();` directly —
  `embassy_sync::channel::Channel::new()` is `const fn`.
- **Don't pull `ui::TextPanel` into the router task.** The display
  task owns the widget; the router only sends commands. This keeps
  the borrow graph trivial.

## Font fidelity (deferred again)

The OEM PCF fonts (PTSans variants in `../MIDICAPTAIN_OEM_BACKUP/
fonts/`) are still on the back burner. embedded-graphics built-in
fonts work; the scene graph isn't locked to a specific font (every
widget takes `&'static MonoFont<'static>`). When you do tackle this:

1. `pcf2bdf` (`apt-get install pcf2bdf` or compile from source).
2. `u8g2-fonts` reads BDF; add it as a dep and swap in.
3. Or bake a custom `MonoFont` from the bitmap data via
   `embedded-graphics`' raw-data constructors.

Don't fold it in alongside other work — it's a session of its own.

## Branch / repo invariants

(Unchanged.)

- Don't modify `../remedy/`, `../webapp/`, `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, or root `../CLAUDE.md`. All firmware work goes in
  `firmware/`.
- Don't expose USB MSC.
- Don't merge to `main`. Stack on `rust-embassy-port`.
- Commit `Cargo.lock`.
- Run `cargo clippy --examples --release -- -D warnings` before
  pushing.

## Concrete deliverable for next session

When you're done:

```powershell
cargo run --release --example display_widgets   # validated on hardware
cargo run --release --bin midicaptain           # NEW — runs the app
```

…and pressing a footswitch causes a text panel on the display to
update via the channel pump. `defmt` log shows the round-trip:

```
[INFO ] buttons: SW0 pressed
[INFO ] router: SW0 press → counter=3
[INFO ] display: ShowCounter(0, 3) → DREW
```

Plus clean clippy, plus an even shorter HANDOFF.md for session five.

## Meta (continuing the previous note)

The previous HANDOFF said "encode forward-looking knowledge in
HANDOFF.md, and let code + commit history + PR description carry the
backward-looking knowledge." That advice held. This file is the
shortest yet because:

- The scene-graph layer is in `src/ui/` — read it, not prose about it.
- The dirty-flag pattern's only design call worth restating is "one
  trait, not Drawable + DirtyTracker split" — see `element.rs`'s
  module doc.
- Session 4 has a clean reference (`../remedy/lib/hardware.py`,
  `../remedy/lib/events.py`, `../remedy/main.py`) for the buttons →
  router → display pipeline.

Same principle applies to your handoff — when the application binary
lands, the next session's HANDOFF can be even shorter.

Good luck.
