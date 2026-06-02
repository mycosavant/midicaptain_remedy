# Handoff — orchestration seed (Wave 3 wrap-up → Wave 4)

This is a **planning seed for an orchestrator**, not a single-task brief.
It gives the next driver the *truth on the ground*, the dependency map for
what's left, and the parallel/serial split — not a linear to-do list. Read
[`ARCHITECTURE.md`](ARCHITECTURE.md) (task graph + channel rules) alongside
it, and [`TESTING.md`](TESTING.md) before validating anything.

> **Currency:** reconciled 2026-06-02 against `main` @ `8b53287`. Since the
> previous edition (@`90fd7d5`/#45) these merged: **#46** (plan refresh),
> **#47** (device-sync receive half — `parse_dt1` + `SYSEX_IN` routed),
> **#48** (active half — RQ1 boot sweep), **#49** (PWM backlight + "Disp
> Bright"), **#50** (`sysex_decode.py`), **#51** (bench findings),
> **#52** (device-sync state persistence — see Finding 1 below). Green gate
> (`cargo build --release --bins --examples` + `cargo clippy --release --lib
> --bins --examples -- -D warnings`) verified on `8b53287`. The "What's
> pending" section below has been trimmed to what actually remains — **if
> anything disagrees with the code, the code wins; fix this file.**

## TL;DR

The port is **functionally complete for the single-device live use case**.
Waves 1–3 landed and merged: the full input→router→output pipeline, the
config/page action system (8 action tiers incl. HID + tap-tempo + cycles +
radio groups), the settings menu, the on-device config editor, the
chromatic tuner (MIDI-fed), flash persistence, a USB-CDC config-sync link,
the PWM backlight, and **Katana device sync** (boot-state read + page-stable
effect toggles — as far as DIN physically allows). What remains is a short
tail: the **encoder-volume** bench triage (Finding 2), a **cross-branch
webapp** currency pass whose firmware side is done, and the **hardware-gated**
audio-DSP tuner. Details below.

## Branch / PR policy — **CHANGED, read this**

The old invariant said "target `SAFE_main`, never `main`." **That no longer
holds.** `SAFE_main` was merged into `main` (commit `cbe0841`,
*"Merge SAFE_main -> main: refresh firmware to the current Rust port"*) and
every feature PR since (#15–#45) has merged to **`main`**. `SAFE_main`
still exists as a branch but is now **legacy and one commit behind `main`**.

- **Target `main`.** It is the single integration branch now.
- The repo is a fork of `nicola-lunghi/hiper-midicaptain`; `gh pr create`
  defaults the base to the *upstream*. Always pass
  `-R mycosavant/midicaptain_remedy --base main` and verify
  `isCrossRepository=false`.
- Develop on a feature branch, one subsystem per PR, open ready-for-review.

## What's landed (truth as of `90fd7d5`)

Everything below is merged to `main`, builds clean, and (except where
noted) has an on-device or self-test proof. File references are the
authoritative source.

| Area | Status | Where |
|---|---|---|
| Channel contracts | ✅ frozen | `src/events.rs` |
| HAL: buttons / encoder / expression / LEDs | ✅ | `src/hal/{buttons,encoder,expression,leds}.rs` |
| MIDI engine: USB+DIN mux, streaming SysEx, Roland/Katana builders | ✅ byte-exact self-test | `src/midi/{mux,sysex,katana}.rs` |
| Flash settings + config store | ✅ coexist self-test | `src/storage.rs` |
| Router + display-mode state machine | ✅ 4 modes (Performance/Menu/Tuner/Edit) | `src/app.rs:113` |
| Config/page action system | ✅ 8 action tiers | `src/config/mod.rs` |
| — incl. radio groups, multi-state cycles, momentary, PC step, CC trigger, tap-tempo, MIDI-thru matrix | ✅ | `src/config/mod.rs` |
| USB-HID (keyboard + consumer control) | ✅ | `src/hal/hid.rs` |
| Display: page-grid performance screen + dark theme + live expr/encoder meters | ✅ HW-validated | `src/ui/page_grid.rs`, `src/display.rs` |
| Scrolling list-view UI | ✅ | `src/ui/list_view.rs` (used by menu + editor) |
| Settings menu + live pedal calibration | ✅ | `src/menu.rs` |
| On-device config editor (`Mode::Edit`) | ✅ action type / CC# / value / colour / cycle steps | `src/editor.rs` |
| Chromatic tuner mode + `TunerView` | ✅ MIDI-fed | `src/tuner.rs`, `src/ui/tuner.rs` |
| YIN pitch detector (fixed-point) | ✅ standalone, self-test | `src/pitch.rs` (not yet wired to a live audio source — see Pending #2) |
| Config-sync wire protocol (COBS + CRC-16) | ✅ `proto_selftest` | `src/proto.rs` (`PROTO_VERSION = 8`) |
| Config sync over USB-CDC (HELLO / GET / SET / hot-reload) | ✅ | `src/bin/midicaptain.rs:356` (`cdc_task`) |
| Boot robustness (display-first, headless fallback, UP+DOWN factory reset) | ✅ HW-validated | `src/bin/midicaptain.rs` |

Self-test examples that gate behaviour: `config_selftest`,
`storage_coexist_selftest`, `proto_selftest`, `pitch_selftest`,
`midi_engine_test`. See `TESTING.md §3`.

## What's pending — the remaining trajectory

The router in `src/app.rs` is the **shared integration surface**.
Everything here is either an *independent module* (new file, feeds the
router through a channel — **parallelisable**) or *integrative* work that
edits the router/app state (**serialise it**). Keep new work in **new
files**; use `isolation: "worktree"` for any agent that must touch shared
files concurrently.

### ▸ Device sync — ✅ as far as DIN allows (#47/#48/#52)

`SYSEX_IN` is routed (`Router::on_sysex_rx`), `katana::parse_dt1` +
`effect_block_cc` decode inbound DT1, the boot RQ1 sweep reads amp type /
preset / the five effect blocks, and effect-toggle state now **survives a page
change** via a device-confirmed cache + an optimistic local-press cache
(`dev_toggles`, `cache_device_toggle`, `reapply_device_state`).

**Hard ceiling found on the bench (2026-06-02):** over **DIN** the Katana
*answers RQ1 reads* (request/response) but does **not** broadcast front-panel
changes, and never echoes a received GA-FC CC. So **live "twist a knob on the
amp → pedal follows" is infeasible over DIN** — the amp only pushes block state
over its USB-host / BOSS-Tone-Studio port, and our pedal is a USB-*device*.
What remains is therefore not "more reflection plumbing" but a topology/poll
question:
- **(optional) RQ1-poll on page entry** — actively *read* the amp's true block
  state when a page is entered, to catch front-panel changes lazily. Only worth
  it **if the amp answers the effect-block RQ1 reads over DIN** (confirmed for
  amp type / preset; block reads unverified — check RTT/`sysex_decode.py` for
  block DT1 replies after the boot sweep first). Edits the router → serial.
- **USB-host bridge** — the only way to get the amp's live BTS broadcasts is to
  sit a USB host between pedal and amp. Hardware/topology, not firmware.

### ▸ Tuner Phase 3 — on-device audio DSP (**hardware-gated**)

The shipped tuner is MIDI-fed, but **the Katana broadcasts nothing over
MIDI for tuning** (documented dead-end — see `git log` for the 2026-05-30
finding). The chosen path is **standalone on-device pitch detection**:
- `src/pitch.rs` (YIN, fixed-point) is **done and self-tested**.
- The analog front-end spec (line-out → AC-couple → mid-rail bias →
  rail-to-rail buffer → ~3.5 kHz anti-alias LPF → GP26/ADC0) is in
  `HARDWARE.md`. **GP26 is free** (`src/pins.rs:148`).
- **Pending:** an `adc_task` that DMA-samples GP26 at ~16 kHz *in
  `Mode::Tuner`*, runs `PitchDetector::detect`, and feeds `TunerView`.
  The ADC is currently owned by `expression_task` — Phase 3 must
  share/duplex it (audio in tuner mode, pedals otherwise).
- **Blocker:** needs the hardware mod to validate. The task can be *written*
  against the spec, but don't claim it works until it's been on real
  silicon with the front-end fitted. Keep the MIDI-receive path too (free
  remote-display fallback from host software).

### ▸ Display brightness — PWM backlight — ✅ done (#49)

The backlight is a PWM slice with a brightness setter, wired to the "Disp
Bright" settings-menu item and persisted to flash.

### ▸ Webapp live-config sync (**cross-branch — already in flight as PR #31**)

The firmware side is done (CDC GET/SET, `proto.rs`). **PR #31** (open,
webapp-only) adds a Web Serial live-config editor under `webapp/`.
- **⚠️ Currency gap, bigger than the PR's own note says.** PR #31 was cut
  2026-05-31 mirroring an early wire format. Since then the firmware's
  `PROTO_VERSION` has reached **8** and `RuntimeConfig` has grown
  (`midi_thru`, `cycles`, appended fields — `src/config/mod.rs:815-898`).
  Before #31 can talk to current firmware its JS codec
  (`webapp/js/device/{postcard,runtimeconfig}.js`) must be re-synced to the
  *current* `config::RuntimeConfig` and accept proto v8. Treat this as a
  required follow-up, not a nit.
- Real end-to-end bench test (Chromium → device CDC → read/tweak/write →
  confirm via RTT) is still pending — #31 was built without a device.
- This is a genuine **cross-branch effort** (firmware config model ⇄ JS
  codec must stay in lockstep). Whenever `config::RuntimeConfig` or
  `PROTO_VERSION` changes, the webapp codec is part of the same change.

## Recommended sequence

The device-sync, PWM-backlight, and `SYSEX_IN` items above are **done**. What's
left, roughly in priority order:

1. **Finding 2 — encoder volume** (bench triage; see below). Smallest, and it's
   blocking a working Katana continuous control.
2. **Webapp #31 currency pass** — re-sync the JS codec to `PROTO_VERSION = 8` /
   current `RuntimeConfig` (`midi_thru`, `cycles`, appended fields), then
   bench-test Chromium → CDC. Cross-branch; firmware side is done.
3. **(optional) Device-sync RQ1-poll on page entry** — only if you want lazy
   front-panel reflection *and* the amp answers block RQ1 over DIN (verify
   first). Otherwise device sync is considered complete for the DIN use case.
4. **Tuner Phase 3** — only once the analog front-end hardware mod exists.

Items 1–2 are independent (different files / branches) and can run in parallel;
any router edit (e.g. #3) stays serial — one PR at a time touching `app.rs`.

## Hardware findings — 2026-06-01 device-sync bench (next session, start here)

Device sync's foundation is **merged and validated on real silicon**: #47
(receive half — `parse_dt1` + `SYSEX_IN` routed), #48 (active half — RQ1 boot
sweep), #49 (PWM backlight), #50 (`scripts/sysex_decode.py`). RTT confirms the
sweep fires on boot (`device query: editor mode + 2 RQ1 read(s) sent`), and
amp-type/preset reflect onto the **Katana** page (page 3) radios. The bench
session surfaced two follow-ups — this is the live to-do.

> GitHub Issues are **disabled** on this repo, so findings live here.

### Finding 1 — effect toggles don't persist across page change / reboot

On **Katana Live** (page 2) A–D are **CC toggles** (`toggle(16)` BOOST,
`toggle(17)` MOD, `toggle(19)` DELAY — `config::PAGE_KATANA_LIVE`). Enable
BOOST, change page and back → LED reads OFF, and it takes **two presses** to
turn off (press 1 re-syncs local state ON, press 2 sends OFF). Three
compounding causes:

1. **Page change clears toggles** by design — `Router::change_page` does
   `self.toggles = [false; 128]` (CLAUDE.md: "cleared on page change").
2. **The sweep doesn't query effect blocks** — `DEVICE_QUERY_SWEEP` covers
   only amp type + preset, not BOOST/MOD/DELAY/REVERB.
3. **No CC↔SysEx bridge.** Effects here are *CC toggles* (CC 16/17/19), but
   the amp reports block state as *SysEx DT1* at `60 00 00 30` (boost),
   `60 00 01 40` (mod), `60 00 05 60` (delay), `60 00 06 10` (reverb) — see the
   `set_boost/set_mod/...` builders. `reflect_sysex` has no path from a block
   DT1 → a CC toggle. The CP firmware bridged these with `cc_alias` in
   `remedy/config/profiles/katana.toml`; the Rust config has no equivalent.

**✅ RESOLVED — #52 (`8b53287`), bench-confirmed.** Implemented: `effect_block_cc`
(the `cc_alias` bridge), the extended block sweep, and a re-applied
device-confirmed cache (`reapply_device_state`). The bench then showed the
sweep/bridge alone weren't enough — over **DIN** the amp neither broadcasts
front-panel changes nor echoes a received CC (see the device-sync section
above), so `dev_toggles` only held the boot value and the press still reset on a
page change. Final fix = an **optimistic local-press cache** (`cache_device_toggle`,
gated by `device_backed = uses_katana_sysex` + `katana::is_effect_cc`): the
pedal records the state *it* commanded, which `reapply_device_state` restores
after a page change. Press BOOST → change page → back → stays ON, one press off.
Live front-panel → pedal reflection remains out of scope on DIN (see above).

### Finding 2 — encoder doesn't drive volume on Katana / Katana Live

Both pages bind `encoder: ContinuousBinding::Sysex(ContinuousSysex::Volume)`.
The code path *looks* correct: `on_encoder` (Performance) → `emit_continuous`
→ `katana::set_volume(scaled)` → `sysex_out`. Needs on-hardware triage with
`sysex_decode.py`:

- `./scripts/midimon.ps1 -Format min-hex | python scripts/sysex_decode.py`,
  turn the encoder:
  - sees `DT1 set Volume = N` → firmware is right; the amp's master-volume
    address (`00 00 04 22`) or required mode is the issue — adjust
    `katana::set_volume`.
  - sees nothing → encoder→router path: confirm `EncoderEvent::Turn` arrives
    in `Mode::Performance` (RTT), the meter (`meter_values[2]`) moves, and
    `enc_value` changes (emit is gated on `v != self.enc_value`).
    `STEPS_PER_DETENT` was retuned to 2 in `4b5d644`.
- Isolate input vs SysEx path: does the encoder drive **CC7** on a
  `MidiCc(7)`-encoder page (the default page)?

## Invariants (do not violate)

- **Target `main`** (see policy section above). One subsystem per PR,
  ready-for-review, `isCrossRepository=false`.
- **Firmware work stays in `firmware/`.** Don't modify `../remedy/`,
  `../webapp/` (except the dedicated webapp effort), `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, or the root `../CLAUDE.md`. `../remedy/lib/` is the
  behavioural reference, read-only.
- **No USB MSC.** The device owns its flash exclusively; host sync rides
  USB-CDC.
- **Commit `Cargo.lock`.** Do **not** commit the `.cargo/config.toml`
  runner flip (probe-rs alt runner is a local, uncommitted change so
  probe-less contributors stay on UF2).
- **Green gate before pushing:**
  `cargo build --release --bins --examples` **and**
  `cargo clippy --release --lib --bins --examples -- -D warnings`.

## Gotchas still live

- **embassy-executor 0.10 spawn idiom:** `spawner.spawn(task(args).unwrap())`
  (the `#[task]` call yields a `Result`; unwrap to the token). No
  `must_spawn`.
- **mipidsi 0.10 ≠ CircuitPython conventions:** offset is constant across
  rotation; this panel needs **colour inversion ON**. Documented in the
  `display.rs` header — don't "fix" it back to the CP numbers.
- **Channels need a `RawMutex`:** `CriticalSectionRawMutex` + a `const`
  `static CH: Channel<...> = Channel::new()`. See the bin.
- **`embassy_futures` tops out at `select4`.** Adding a fifth+ input means
  nesting selects (already done once for `config_req`; `SYSEX_IN` makes a
  third arm). Receive futures are cancellation-safe, so the losing branch
  re-arms with no lost messages.
- **USB is a 5-interface composite** (MIDI ×2, CDC ×2, HID ×1) — hence
  `max-interface-count-8` in `Cargo.toml`. Adding another interface eats
  the headroom.
- **Flash buffer sizing:** the settings+config store buffer is sized to the
  *largest* map item (`MAX_SERIALIZED_LEN`); growing `RuntimeConfig` past
  it will silently fail to persist. `storage_coexist_selftest` guards this.
- Older display-driver / bootstrap gotchas: `git log -p firmware/HANDOFF.md`.

## Toolchain reality

- **Build anywhere; flash only from Windows PowerShell** (WSL can't see
  USB; the probe + `probe-rs.exe` live on Windows). `cargo run` uses the
  runner in `.cargo/config.toml` (UF2 default; probe-rs is the local,
  uncommitted alt). Full flashing playbook in `TESTING.md §4`.

## Meta

Keep this file forward-looking: hand the next driver the dependency map and
the parallel/serial split, and let code + commits carry the backward-looking
detail. The foundation is solid and largely validated — what's left is a
short, well-scoped tail. Good luck.
