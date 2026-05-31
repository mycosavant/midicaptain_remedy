//! Quadrature rotary encoder + debounced push-button.
//!
//! Wraps the three encoder GPIOs (phase A, phase B, push-switch) and emits
//! [`EncoderEvent`]s: one [`Turn`](EncoderEvent::Turn) per mechanical
//! detent and [`Press`](EncoderEvent::Press) / [`Release`](EncoderEvent::Release)
//! for the push-button. Pins are fixed by the board map in
//! [`crate::pins`]: `ENCODER_A_PIN = 2`, `ENCODER_B_PIN = 3`,
//! `ENCODER_SW_PIN = 0`.
//!
//! # Design
//!
//! Interrupt-driven, not polled. [`Encoder::next_event`] parks on
//! [`select3`] across `wait_for_any_edge` for all three pins, so the task
//! consumes zero CPU while the knob is still and reacts within an
//! interrupt latency of any movement.
//!
//! ## Quadrature decode
//!
//! A and B form a 2-bit Gray code (`state = A<<1 | B`). On every edge we
//! read the *current* level of **both** pins (not just the one that woke
//! us) and look up the `(previous, current)` transition in [`QDEC_LUT`],
//! which yields `+1` / `-1` for a valid single-step transition and `0`
//! for the idle case or an illegal double-step (mechanical bounce). Steps
//! accumulate; one mechanical detent is [`STEPS_PER_DETENT`] quadrature
//! transitions, so a whole detent is emitted only once the accumulator
//! crosses that threshold. This is the same "one count per detent"
//! behaviour CircuitPython's `rotaryio.IncrementalEncoder` gave the
//! reference firmware (`remedy/lib/hardware.py::Encoder`, default
//! `divisor = 4`).
//!
//! ## Acceleration
//!
//! Ported from `remedy/lib/hardware.py::Encoder.get_delta`: when detents
//! arrive in quick succession the emitted delta is multiplied so a fast
//! spin covers more ground. `< 50 ms` between detents → ×4, `< 100 ms` →
//! ×2, otherwise ×1. Enabled by default; toggle with
//! [`Encoder::set_acceleration`].
//!
//! ## Button debounce
//!
//! On any edge of the switch line we wait out the contact bounce
//! ([`DEBOUNCE`]) and then sample the settled level once. An edge whose
//! settled level matches the last reported state (a bounce that returned
//! to rest) is dropped; only a real transition emits `Press` / `Release`.
//! The switch is active-LOW with an internal pull-up, matching the
//! footswitches (`pins.rs`): `Level::Low` == pressed.
//!
//! # Direction sign
//!
//! "Clockwise" is a wiring convention, not a physical law. With the LUT
//! below, the rotation that walks the Gray code `00→01→11→10` reads as
//! negative. If a bring-up test shows the knob counts backwards, either
//! swap the A/B pins at the call site or negate `step` in
//! [`Encoder::decode_edge`] — do **not** paper over it downstream.

use embassy_futures::select::{select3, Either3};
use embassy_rp::gpio::{Input, Pin, Pull};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Timer};

use crate::events::EncoderEvent;

/// Quadrature transitions per mechanical detent. Standard detented
/// encoders complete one full Gray-code cycle (4 edges) between adjacent
/// detents. Lower this to `2` or `1` for the rare half-/quarter-detent
/// part.
pub const STEPS_PER_DETENT: i8 = 4;

/// Push-button debounce window. 8 ms sits comfortably past typical tactile
/// contact bounce (≈1–5 ms) while staying well under human perception, so
/// presses feel instant. Tunable in the 5–15 ms range.
const DEBOUNCE: Duration = Duration::from_millis(8);

/// Inter-detent gap below which a turn is "fast" → ×4 (see module docs).
const ACCEL_FAST_MS: u64 = 50;
/// Inter-detent gap below which a turn is "medium" → ×2.
const ACCEL_MED_MS: u64 = 100;

/// Gray-code transition lookup. Index is `(prev_state << 2) | curr_state`
/// where `state = A<<1 | B`. `+1`/`-1` are the two valid single-step
/// directions; `0` is "no movement" (same state) or an illegal
/// double-bit jump (lost edge / bounce), which is intentionally ignored.
const QDEC_LUT: [i8; 16] = [
    0, -1, 1, 0, //
    1, 0, 0, -1, //
    -1, 0, 0, 1, //
    0, 1, -1, 0, //
];

/// A rotary encoder with quadrature decode, detent accumulation,
/// velocity-based acceleration and a debounced push-button.
///
/// Construct once with [`Encoder::new`], then `await` [`Encoder::next_event`]
/// in a loop — typically inside a dedicated task that forwards the events
/// to the router over an `embassy_sync` channel.
pub struct Encoder<'d> {
    a: Input<'d>,
    b: Input<'d>,
    sw: Input<'d>,
    /// Last decoded 2-bit quadrature state (`A<<1 | B`).
    state: u8,
    /// Sub-detent step accumulator; ranges within `±STEPS_PER_DETENT`.
    accum: i8,
    /// Timestamp of the previous emitted detent, for acceleration.
    last_detent: Option<Instant>,
    /// Last *reported* button state (`true` == pressed). Seeded from the
    /// pin at construction so the first edge is judged correctly.
    pressed: bool,
    /// Whether velocity-based acceleration is applied to `Turn` deltas.
    accel: bool,
}

impl<'d> Encoder<'d> {
    /// Build an encoder from the three raw GPIOs.
    ///
    /// Internal pull-ups are enabled on all three lines: the quadrature
    /// outputs are open-drain on most modules and the switch is wired to
    /// ground (active-LOW). Pass the pins in `(A, B, switch)` order —
    /// `pins::ENCODER_A_PIN`, `pins::ENCODER_B_PIN`, `pins::ENCODER_SW_PIN`.
    ///
    /// Acceleration is enabled by default (matching the CircuitPython
    /// reference); disable it with [`Encoder::set_acceleration`].
    pub fn new(a: Peri<'d, impl Pin>, b: Peri<'d, impl Pin>, sw: Peri<'d, impl Pin>) -> Self {
        let a = Input::new(a, Pull::Up);
        let b = Input::new(b, Pull::Up);
        let sw = Input::new(sw, Pull::Up);

        // Seed state from the live pins so the first real edge produces a
        // genuine transition rather than a phantom step from a fixed 0.
        let state = ((a.is_high() as u8) << 1) | (b.is_high() as u8);
        let pressed = sw.is_low(); // active-LOW

        Self {
            a,
            b,
            sw,
            state,
            accum: 0,
            last_detent: None,
            pressed,
            accel: true,
        }
    }

    /// Enable or disable velocity-based acceleration of `Turn` deltas.
    pub fn set_acceleration(&mut self, enabled: bool) {
        self.accel = enabled;
    }

    /// Wait for and return the next encoder event.
    ///
    /// Resolves on the first of: a completed detent of rotation, or a
    /// debounced button edge. Internally loops over raw GPIO edges,
    /// swallowing sub-detent motion and bounce, so every value it returns
    /// is a real, user-visible event.
    pub async fn next_event(&mut self) -> EncoderEvent {
        loop {
            match select3(
                self.a.wait_for_any_edge(),
                self.b.wait_for_any_edge(),
                self.sw.wait_for_any_edge(),
            )
            .await
            {
                // A or B moved: re-read both lines and decode.
                Either3::First(()) | Either3::Second(()) => {
                    if let Some(ev) = self.decode_edge() {
                        return ev;
                    }
                }
                // Switch line moved: debounce, then report any net change.
                Either3::Third(()) => {
                    if let Some(ev) = self.debounce_button().await {
                        return ev;
                    }
                }
            }
        }
    }

    /// Fold one quadrature edge into the accumulator, returning a `Turn`
    /// when a whole detent has been traversed.
    fn decode_edge(&mut self) -> Option<EncoderEvent> {
        let curr = ((self.a.is_high() as u8) << 1) | (self.b.is_high() as u8);
        let step = QDEC_LUT[((self.state << 2) | curr) as usize & 0x0f];
        self.state = curr;

        if step == 0 {
            return None; // idle re-read or illegal transition — ignore
        }
        self.accum += step;

        if self.accum >= STEPS_PER_DETENT {
            self.accum -= STEPS_PER_DETENT;
            Some(EncoderEvent::Turn(self.detent_delta(1)))
        } else if self.accum <= -STEPS_PER_DETENT {
            self.accum += STEPS_PER_DETENT;
            Some(EncoderEvent::Turn(self.detent_delta(-1)))
        } else {
            None
        }
    }

    /// Apply acceleration to a unit detent in direction `dir` (`±1`),
    /// returning the signed delta to report. Updates the detent clock.
    fn detent_delta(&mut self, dir: i8) -> i8 {
        let now = Instant::now();
        let mult = if self.accel {
            match self.last_detent {
                Some(prev) => match now.saturating_duration_since(prev).as_millis() {
                    ms if ms < ACCEL_FAST_MS => 4,
                    ms if ms < ACCEL_MED_MS => 2,
                    _ => 1,
                },
                None => 1, // first detent since boot: no reference, no boost
            }
        } else {
            1
        };
        self.last_detent = Some(now);
        dir * mult
    }

    /// Debounce a switch-line edge: settle, sample once, and emit only on
    /// a net change from the last reported state.
    async fn debounce_button(&mut self) -> Option<EncoderEvent> {
        Timer::after(DEBOUNCE).await;
        let pressed = self.sw.is_low(); // active-LOW: low == pressed
        if pressed == self.pressed {
            return None; // bounce that returned to rest — no net change
        }
        self.pressed = pressed;
        Some(if pressed {
            EncoderEvent::Press
        } else {
            EncoderEvent::Release
        })
    }
}

// ── Channel aliases + task ─────────────────────────────────────────────
// Mirrors the `expression`/`leds` modules: the app owns a `static
// EncoderChannel`, hands the receiver to the router and the sender to the
// task.

/// Depth of the [`EncoderEvent`] channel between the task and the router.
/// Detents/presses are low-rate human input; 8 absorbs a fast spin without
/// the task ever back-pressuring (it would just park on the next edge).
pub const ENCODER_QUEUE_DEPTH: usize = 8;

/// Bounded MPSC channel carrying [`EncoderEvent`]s to the router.
pub type EncoderChannel = Channel<CriticalSectionRawMutex, EncoderEvent, ENCODER_QUEUE_DEPTH>;
/// Sender half — held by [`encoder_task`].
pub type EncoderSender = Sender<'static, CriticalSectionRawMutex, EncoderEvent, ENCODER_QUEUE_DEPTH>;
/// Receiver half — held by the router.
pub type EncoderReceiver = Receiver<'static, CriticalSectionRawMutex, EncoderEvent, ENCODER_QUEUE_DEPTH>;

/// Drive an [`Encoder`] forever, forwarding every event to the router.
///
/// `next_event` already swallows sub-detent quadrature and button bounce,
/// so every value sent here is a real, user-visible event. Block-the-
/// producer on send: encoder events are navigation/menu *intent* we don't
/// want to drop, and the queue depth makes blocking safe.
#[embassy_executor::task]
pub async fn encoder_task(mut encoder: Encoder<'static>, sender: EncoderSender) {
    loop {
        let ev = encoder.next_event().await;
        sender.send(ev).await;
    }
}
