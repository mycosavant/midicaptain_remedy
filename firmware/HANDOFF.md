# Handoff — next session: ORCHESTRATION + full-scope planning

This handoff is different from the previous ones. The display stack is
hardware-validated and the application skeleton exists, so the port has
crossed from "prove the toolchain" into "build out subsystems." The next
session's job is **planning and orchestration**: lay out the remaining
trajectory, then drive it — parallel subagents / parallel sessions for
genuinely independent subsystems, serialised integration for the shared
router. This file hands you the map to start from.

Read `ARCHITECTURE.md` (task graph + channel rules) alongside this.

## Where things stand (Wave 1 merged to `SAFE_main` @ `d7c626a`)

**Wave 1 is complete in module form** (PRs #6–#11, all merged to
`SAFE_main`). The channel contract `src/events.rs` is frozen and on
`SAFE_main`; the orchestration hiccup where it briefly missed the branch
has been fully reconciled (single definition per type, green gate clean).
Landed since the original handoff: the `events` contract, the HAL tasks
(`hal/{encoder,expression,leds}`), the MIDI engine
(`midi/{mux,sysex,katana}`), and the flash settings store (`storage`) —
each with a proof example. **None of these are wired into the router yet**
— that's Wave 2 (below).

**Hardware-validated on real silicon** (Pi Debug Probe + probe-rs):
- ST7789 display: geometry (`Deg0` + `display_offset(0,0)`), colour
  inversion ON (`ColorInversion::Inverted`), 24 MHz SPI. Splash + widgets
  render correctly, upright, centred, smooth.
- SWD debug access: 3-pad bottom-side header (square=SWCLK, mid=GND,
  far=SWDIO) — see `HARDWARE.md`. `probe-rs run` flashes + streams RTT.
- UI scene graph: `Widget` trait, const `Palette`, `ValueBar`
  (delta-paint, no flicker), `TextPanel`. Dirty-flag gating proven via
  `examples/display_widgets.rs`.
- **Application skeleton**: `src/bin/midicaptain.rs` —
  buttons → router → display over `embassy_sync` channels. Compiles
  clean; **not yet flashed** (built right before this handoff).

**Toolchain reality:**
- Build anywhere; **flash only from Windows PowerShell** (WSL can't see
  USB; probe-rs.exe + the probe live on Windows). `cargo run` uses the
  runner in `.cargo/config.toml`.
- The probe-rs runner flip in `.cargo/config.toml` is a **local,
  uncommitted** change (repo default stays UF2 so probe-less contributors
  aren't broken). Don't `git add` that file.

**First action for next session on hardware:** flash the app skeleton and
confirm the pipeline:
```powershell
cargo run --release --bin midicaptain
```
Press footswitches → the status panel should show `<name> x<count>` and
RTT should log `router: A pressed -> count=N` / `display: A xN`. That
validates the channel architecture before fanning out work onto it.

## Trajectory

The router in `bin/midicaptain.rs` is the **shared integration surface**.
Everything else is either an *independent module* that feeds the router
through a channel (parallelisable), or *integrative* work that edits the
router/app state (serialise it).

### ✅ Wave 1 — DONE (merged to `SAFE_main`)

Step 0 (freeze `src/events.rs` channel contracts) and the five independent
modules all landed, each with a proof example, gate green:

| Module | Files | Status |
|---|---|---|
| Channel contracts | `src/events.rs` | ✅ frozen, single def per type |
| WS2812 LEDs | `src/hal/leds.rs` | ✅ `leds_task` consumes `LedFrame`; 10→30 px fan-out, `idle_dim` |
| MIDI engine | `src/midi/{mux,sysex,katana}.rs` | ✅ USB+DIN mux, streaming SysEx, Roland/Katana builders (byte-exact self-test) |
| Encoder | `src/hal/encoder.rs` | ✅ IRQ quadrature + accel + debounced push |
| Expression | `src/hal/expression.rs` | ✅ async-ADC pedals + calibration map + wizard |
| Storage | `src/storage.rs` | ✅ flash KV settings store (direct async accessor) |

None of these are wired into the router yet — that's Wave 2.

### ✅ Wave 2 — DONE (merged + hardware-validated)

Integration + the config/page system landed across phased commits (PRs
#12/#13 + boot-robustness #14), all merged to `SAFE_main`:
- All five Wave-1 modules wired into the bin behind one `bind_interrupts!`;
  `Router` (in `src/app.rs`) drives config/page action dispatch (CC/PC/
  SysEx/page-nav), per-CC toggle state + LED feedback, bidirectional CC
  sync, and `DisplayCmd` Page/Action modes; `select4` over the four input
  channels. Footswitch scanner lifted to `src/hal/buttons.rs`; bin is thin
  wiring.
- Boot robustness: display task spawns first ("booting…" splash); display
  init is non-fatal (headless fallback); hold **UP+DOWN at power-on** →
  `Storage::factory_reset()`.
- **Validated on real silicon** (probe-rs/SWD): clean boot, footswitch →
  page-nav + A–D toggle LEDs, flash settings persist across reboot.

Chosen decisions still in force: router fan-in = `select4`; config v1 =
baked-in Rust consts; USB stood up composite-capable (MidiClass now).

### ▶ Wave 3 — display modes / features (current focus)

Features (CP refs): **settings menu** (`menu.py`), **tuner** (`tuner.py`),
**device sync** (Katana RQ1→LED states), **webapp sync**.

**Step 0 — foundation (serial; freeze first):**
- `events::MidiRx::PitchBend { channel, value }` + decode `0xE0` in
  `midi/mux.rs` — the tuner reads the note (Note On) + cents (Pitch Bend)
  from the amp; the contract currently drops pitch-bend. *(Additive —
  landed in `feat/wave3-foundation`.)*
- **Display-mode state machine** in `app::Router` — landed as `Mode {
  Performance, Menu }` (Tuner adds a variant later); input routing is
  mode-dependent; the display task renders menu/cal screens via
  `DisplayCmd::Menu`/`Cal`. *(Landed with the menu.)*
- **Encoder long-press** in the Router (hold → enter/exit menu). *(Landed.)*
- **`Storage` → `Router`** — moved in; `save()` awaits in the async handlers.
  *(Landed.)*
- **Route `SYSEX_IN` into the router** (`select4`→`select5`) — still pending;
  produced but unconsumed; device-sync needs the Katana DT1 responses.

**Per-feature work (parallelisable on the foundation):**
- *Settings menu* — **LANDED** (`src/menu.rs`, encoder-driven, single-item
  view). Items: MIDI channel, LED brightness, calibrate pedal 1/2, exit.
  Enter via encoder long-press. **Live calibration** works: the wizard reads
  the sampler's published raw via `expression::LATEST_RAW` and pushes the new
  endpoints back via `expression::LIVE_CAL` (applied without a reboot) +
  persists to flash. LED brightness scales the frame in `app::scale`.
  *Deferred:* **display brightness** (needs a PWM backlight — `display.rs` is
  still GPIO-high) and live-updating raw readout while moving the pedal (the
  raw shown refreshes only on input events, CP-faithful).
- *Tuner* — **LANDED** (`src/tuner.rs` state + `src/ui/tuner.rs` `TunerView`).
  Cooperative: `TunerToggle` (long-press **D** on either page) sends CC#25 = 127
  and enters `Mode::Tuner`; the amp streams Note On + Pitch Bend, which
  `Router::on_midi_rx_tuner` maps to note + cents and pushes as
  `DisplayCmd::Tuner`. `TunerView` paints the note name, cents text, and a
  needle bar (green in-tune / yellow close / red sharp / blue flat), delta-
  painting the needle. Any footswitch release — or an encoder hold — sends
  CC#25 = 0 and returns to performance. The display task wipes the screen on
  the Normal↔Tuner layout switch (`Screen` enum). *Deferred:* a larger note
  glyph (only `FONT_10X20` is built-in) and cents smoothing (CP averaged 3).
- *Device sync* — boot/connect RQ1 sweep + a Katana DT1 response parser in
  `midi/katana.rs`; updates toggle LED states.
- *Webapp sync — DEFERRED.* The existing webapp (on `main`, not the port) is
  a static **OEM-format preset *file* editor** (SuperMode `page*.txt`,
  GeekMode `gekey*.dat`) that exports via file download + Web-MIDI preview —
  it has **no USB-serial path**, and the port has **no USB MSC**. Real sync
  would need the webapp to also learn the Rust config + a WebSerial transport
  (a cross-branch effort). Scope it only when that's on the table.

Recommended sequence: foundation → settings menu (retires the UP+DOWN reset
hack) → tuner → **device sync (next)**. Menu + tuner both landed as
self-contained modes (own state module + `*View` widget) on the shared
display-mode surface — follow the same shape for device sync. Its one
remaining foundation dependency is the `SYSEX_IN`→router `select4`→`select5`
wiring noted above, plus a Katana DT1 response parser in `midi/katana.rs`.

A note on cost: parallel subagents editing the *same* files conflict. Keep
new work in **new files** and integration **serial**. Use
`isolation: "worktree"` for any agent that must touch shared files
concurrently.

## Invariants (do not violate)

- **Branch/PR:** target `SAFE_main`, **never `main`**. The repo is a
  **fork of `nicola-lunghi/hiper-midicaptain`** — `gh pr create` defaults
  the base to the *upstream*. `gh repo set-default mycosavant/midicaptain_remedy`
  is already set; still pass `-R mycosavant/midicaptain_remedy --base
  SAFE_main` explicitly and verify `isCrossRepository=false`. (See the
  `project-rust-port-branching` memory.)
- **Don't modify** `../remedy/`, `../webapp/`, `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, root `../CLAUDE.md`. Firmware work stays in `firmware/`.
- **No USB MSC.** The device owns its flash exclusively.
- **Commit `Cargo.lock`.** Don't commit the `.cargo/config.toml` runner
  flip.
- **Green gate before pushing:**
  `cargo build --release --bins --examples` and
  `cargo clippy --release --bins --examples -- -D warnings` (note
  `--bins` — there's a binary now).

## Gotchas still live

- embassy-executor 0.10 spawn idiom: `spawner.spawn(task(args).unwrap())`
  (the `#[task]` call yields a Result; unwrap to the token). No
  `must_spawn`.
- mipidsi 0.10 ≠ CircuitPython conventions: offset is constant across
  rotation; this panel needs colour inversion ON. (Documented in
  `display.rs` header — don't "fix" it back to the CP numbers.)
- Channels need a `RawMutex`: `CriticalSectionRawMutex` +
  `static CH: Channel<...> = Channel::new()` (const). See the bin.
- Older display-driver / bootstrap gotchas: `git log -p firmware/HANDOFF.md`.

## Meta

Previous handoffs encoded forward-looking knowledge here and let code +
commits carry the backward-looking detail. This one widens that: it's a
*planning seed* for an orchestrator, not a single-task brief. Keep that
framing if you re-hand-off — give the next driver the dependency map and
the parallel/serial split, not a linear to-do list.

Good luck — the foundation is solid and validated; it's build-out from here.
