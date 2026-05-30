//! Expression-pedal ADC sampling → [`ExprEvent`].
//!
//! Two TRS expression jacks feed the RP2040 SAR ADC: **GP27 = pedal 0**
//! (ADC1) and **GP28 = pedal 1** (ADC2), against the internal 3.3 V
//! reference (see `pins.rs` / `HARDWARE.md`). GP29's optional battery
//! divider is ignored here. [`expression_task`] samples both at ~100 Hz,
//! smooths, maps through calibration to a 0..=127 value, and emits an
//! [`ExprEvent`] only when that value *changes*.
//!
//! ## Ported behaviour
//!
//! This is the Rust port of `remedy/lib/hardware.py::ExpressionPedal` and
//! the 3-step calibration wizard in `remedy/lib/menu.py`. The CircuitPython
//! firmware sampled a 16-bit ADC each main-loop pass and reported a value
//! only when it moved by ≥1 LSB of the 0..127 output ("dirty" gating); it
//! applied a calibrated min/max, a 2 % end deadzone, and an optional
//! response curve. We keep all of that and add **light moving-average
//! smoothing plus a raw-domain hysteresis deadband**, because the RP2040
//! ADC is only 12-bit and noisier than CP's effective resolution, and the
//! async task samples faster than the CP loop did.
//!
//! Everything is integer-only: no `f32`/`libm` on the Cortex-M0+ (which has
//! no FPU), so mapping and curves are deterministic and cheap.
//!
//! ## Calibration persistence (dependency)
//!
//! Calibration here is a runtime STUB: it defaults to the full ADC span
//! (`0..=4095`) and is set via [`ExpressionInputs::set_calibration`] /
//! [`PedalProcessor::set_calibration`]. Persisting it across reboots is a
//! separate flash-storage workstream (`src/storage/`, planned in
//! `ARCHITECTURE.md`) — the CP firmware used the RP2040 NVM only because
//! its filesystem was read-only. [`CalibrationWizard`] ports the 3-step
//! capture semantics; wiring it to live pedal readings and to flash is left
//! to the menu + storage workstreams. This module does not block on them.

use core::cell::Cell;

use crate::events::ExprEvent;
use defmt::warn;
use embassy_rp::adc::{Adc, AdcPin, Async, Channel as AdcChannel};
use embassy_rp::gpio::Pull;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Ticker};

/// Number of expression pedals (GP27, GP28).
pub const PEDAL_COUNT: usize = 2;

/// Full-scale reading of the RP2040 12-bit SAR ADC.
pub const ADC_FULL_SCALE: u16 = 4095;

/// Maximum mapped output (MIDI 7-bit range is `0..=127`).
pub const MIDI_MAX: u8 = 127;

/// Per-pedal sample rate. Both pedals are read once per tick, so each is
/// sampled at this rate. ~100 Hz is smooth for a foot pedal without
/// flooding the ADC or the event channel.
pub const SAMPLE_RATE_HZ: u64 = 100;

/// Depth of the [`ExprEvent`] channel between the task and the router.
/// Pedal motion coalesces to one event per changed value; 8 absorbs a fast
/// sweep without back-pressuring the sampler.
pub const EXPR_QUEUE_DEPTH: usize = 8;

/// Light moving-average window over raw samples. 4 samples ≈ 40 ms at
/// [`SAMPLE_RATE_HZ`] — enough to halve ADC noise without sluggish feel.
const SMOOTH_WINDOW: usize = 4;

/// Raw-count hysteresis deadband. The smoothed reading must move at least
/// this far from where we last evaluated before we reconsider the output.
/// One 0..=127 step spans `ADC_FULL_SCALE / 127 ≈ 32` counts, so 8 (a
/// quarter-step) suppresses boundary flicker while keeping full resolution.
const HYSTERESIS_COUNTS: u16 = 8;

/// End deadzone, percent of the calibrated span, clamped to 0/127 at each
/// extreme. Ported from `ExpressionPedal.deadzone` (CP default 2 %).
const DEADZONE_PERCENT: u32 = 2;

// ── Channel aliases ────────────────────────────────────────────────────
// The app owns the `static ExprChannel` and hands the receiver to its
// router; the sampler gets the sender.

/// Bounded MPSC channel carrying [`ExprEvent`]s from the sampler to the
/// router.
pub type ExprChannel = Channel<CriticalSectionRawMutex, ExprEvent, EXPR_QUEUE_DEPTH>;
/// Sender half of an [`ExprChannel`] — held by [`expression_task`].
pub type ExprSender = Sender<'static, CriticalSectionRawMutex, ExprEvent, EXPR_QUEUE_DEPTH>;
/// Receiver half of an [`ExprChannel`] — held by the router.
pub type ExprReceiver = Receiver<'static, CriticalSectionRawMutex, ExprEvent, EXPR_QUEUE_DEPTH>;

// ── Calibration handshake (settings menu ↔ sampler task) ────────────────
// Two tiny shared cells, not channels: the menu wants the *latest* raw
// reading (not a queue), and pushes a new calibration the sampler applies on
// its next tick. Blocking critical-section mutexes — both accessors are
// synchronous and the critical sections are a single load/store.

/// Latest raw ADC reading per pedal, published by [`expression_task`] every
/// sample. The calibration wizard reads this to capture min/max endpoints.
pub static LATEST_RAW: BlockingMutex<CriticalSectionRawMutex, Cell<[u16; PEDAL_COUNT]>> =
    BlockingMutex::new(Cell::new([0; PEDAL_COUNT]));

/// A calibration pushed by the menu; [`expression_task`] applies it (live,
/// without a reboot) on its next tick and clears it. `None` = nothing pending.
pub static LIVE_CAL: BlockingMutex<CriticalSectionRawMutex, Cell<Option<[Calibration; PEDAL_COUNT]>>> =
    BlockingMutex::new(Cell::new(None));

// ── Calibration & curve ────────────────────────────────────────────────

/// Calibrated raw-ADC endpoints for a pedal. `min` maps to output 0, `max`
/// to 127. Defaults to the full ADC span until the storage workstream
/// loads a saved calibration.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Calibration {
    /// Raw ADC reading at the heel (output 0).
    pub min: u16,
    /// Raw ADC reading at the toe (output 127).
    pub max: u16,
}

impl Calibration {
    /// Uncalibrated default spanning the whole 12-bit range.
    pub const DEFAULT: Self = Self {
        min: 0,
        max: ADC_FULL_SCALE,
    };

    /// A calibration is usable only if the toe reads above the heel.
    /// Mirrors the `cal_max > cal_min` guard in `menu.py`.
    pub const fn is_valid(&self) -> bool {
        self.max > self.min
    }
}

impl Default for Calibration {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Pedal response curve. Ported from `ExpressionPedal.curve`.
#[derive(Clone, Copy, PartialEq, Eq, Default, defmt::Format)]
pub enum Curve {
    /// Output tracks position 1:1 (CP default).
    #[default]
    Linear,
    /// Audio/log taper — more resolution near the heel: `sqrt(x)`.
    Log,
    /// Exponential taper — more resolution near the toe: `x²`.
    Exp,
}

impl Curve {
    /// Apply the curve to an already-scaled `0..=127` value. The non-linear
    /// shapes are evaluated with integer arithmetic (`v` is small, so
    /// `v*127` never overflows `u32` and the results stay in `0..=127`):
    /// `Exp = v²/127`, `Log = isqrt(v*127)`.
    fn apply(self, v: u8) -> u8 {
        match self {
            Curve::Linear => v,
            Curve::Exp => ((v as u32 * v as u32) / MIDI_MAX as u32) as u8,
            Curve::Log => (v as u32 * MIDI_MAX as u32).isqrt() as u8,
        }
    }
}

// ── Per-pedal signal processing (pure, hardware-free) ──────────────────

/// All the per-pedal state: smoothing window, calibration, curve, and the
/// dirty-gating bookkeeping. Pure and hardware-free — [`push`] takes a raw
/// ADC reading and returns the mapped value *iff* it changed.
///
/// [`push`]: PedalProcessor::push
pub struct PedalProcessor {
    cal: Calibration,
    curve: Curve,
    // Moving average over the last `count` (≤ SMOOTH_WINDOW) raw samples.
    window: [u16; SMOOTH_WINDOW],
    head: usize,
    count: usize,
    sum: u32,
    // Dirty-gating: the smoothed reading at the last evaluation, and the
    // last emitted output (None until the first sample establishes both).
    anchor: u16,
    last_value: Option<u8>,
}

impl PedalProcessor {
    /// A processor with default calibration and a linear curve.
    pub const fn new() -> Self {
        Self {
            cal: Calibration::DEFAULT,
            curve: Curve::Linear,
            window: [0; SMOOTH_WINDOW],
            head: 0,
            count: 0,
            sum: 0,
            anchor: 0,
            last_value: None,
        }
    }

    /// Replace the calibration endpoints. Rejects an invalid range
    /// (`max ≤ min`, after clamping `max` to the ADC full scale), keeping
    /// the previous calibration and returning `false` — same guard as
    /// `menu.py`'s NVM-load. On success the next [`push`] re-emits under the
    /// new mapping.
    ///
    /// This is the stub calibration entry point; the flash-storage
    /// workstream will call it on boot with persisted values.
    ///
    /// [`push`]: PedalProcessor::push
    pub fn set_calibration(&mut self, min: u16, max: u16) -> bool {
        let max = max.min(ADC_FULL_SCALE);
        if max <= min {
            warn!("expr: ignoring invalid calibration (min={}, max={})", min, max);
            return false;
        }
        self.cal = Calibration { min, max };
        self.last_value = None; // force a fresh emit under the new mapping
        true
    }

    /// Current calibration.
    pub fn calibration(&self) -> Calibration {
        self.cal
    }

    /// Set the response curve. Forces a re-emit on the next [`push`].
    ///
    /// [`push`]: PedalProcessor::push
    pub fn set_curve(&mut self, curve: Curve) {
        self.curve = curve;
        self.last_value = None;
    }

    /// The last emitted output, or `None` before the first sample.
    pub fn value(&self) -> Option<u8> {
        self.last_value
    }

    /// Feed one raw ADC reading. Returns `Some(value)` when the smoothed,
    /// calibrated `0..=127` output changes (so the caller emits an
    /// [`ExprEvent`]), or `None` when it's unchanged or still inside the
    /// hysteresis deadband.
    pub fn push(&mut self, raw: u16) -> Option<u8> {
        let smoothed = self.smooth(raw);

        match self.last_value {
            // First sample: establish the baseline and emit it so the
            // router learns the pedal's starting position (CP's
            // `get_if_changed` returns the value on first call).
            None => {
                let v = self.map(smoothed);
                self.anchor = smoothed;
                self.last_value = Some(v);
                Some(v)
            }
            Some(prev) => {
                // Ignore jitter within the deadband around the last
                // evaluation point.
                if smoothed.abs_diff(self.anchor) < HYSTERESIS_COUNTS {
                    return None;
                }
                self.anchor = smoothed;
                let v = self.map(smoothed);
                if v != prev {
                    self.last_value = Some(v);
                    Some(v)
                } else {
                    None
                }
            }
        }
    }

    /// Update the moving average with a new sample and return the current
    /// average over the `count` most-recent samples (unbiased during the
    /// warm-up before the window fills).
    fn smooth(&mut self, raw: u16) -> u16 {
        if self.count == SMOOTH_WINDOW {
            self.sum -= self.window[self.head] as u32; // evict oldest
        } else {
            self.count += 1;
        }
        self.window[self.head] = raw;
        self.sum += raw as u32;
        self.head = (self.head + 1) % SMOOTH_WINDOW;
        (self.sum / self.count as u32) as u16
    }

    /// Map a (smoothed) raw reading to `0..=127` through calibration, the
    /// end deadzone, and the response curve — the integer port of
    /// `ExpressionPedal.value`.
    fn map(&self, raw: u16) -> u8 {
        let Calibration { min, max } = self.cal;
        if max <= min {
            return 0; // invalid calibration: fail safe to the heel
        }
        let span = (max - min) as u32; // > 0
        let num = (raw.clamp(min, max) - min) as u32; // 0..=span
        let dz = span * DEADZONE_PERCENT / 100;

        let value = if num <= dz {
            0
        } else if num >= span - dz {
            MIDI_MAX as u32
        } else {
            // Rescale the live region [dz, span-dz] onto 0..=127, rounded.
            let eff = (span - 2 * dz).max(1);
            ((num - dz) * MIDI_MAX as u32 + eff / 2) / eff
        };

        self.curve.apply(value.min(MIDI_MAX as u32) as u8)
    }
}

impl Default for PedalProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 3-step calibration wizard (ported from menu.py) ────────────────────

/// Where a [`CalibrationWizard`] is in the capture sequence.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum CalPhase {
    /// Not calibrating.
    Idle,
    /// Waiting to capture the heel (minimum) position.
    AwaitMin,
    /// Waiting to capture the toe (maximum) position.
    AwaitMax,
    /// Both endpoints captured; result is ready (if valid).
    Complete,
}

/// The 3-step "set min → set max → confirm" calibration wizard from
/// `menu.py` (`handle_calibration_button`), as a hardware-free state
/// machine. The menu workstream drives it: start it for a pedal, then on
/// each confirm press feed the pedal's *current* raw reading via
/// [`advance`]; when it reaches [`CalPhase::Complete`], hand [`result`] to
/// [`ExpressionInputs::set_calibration`] and to flash for persistence.
///
/// [`advance`]: CalibrationWizard::advance
/// [`result`]: CalibrationWizard::result
pub struct CalibrationWizard {
    phase: CalPhase,
    pedal: usize,
    min: u16,
    max: u16,
}

impl CalibrationWizard {
    /// An idle wizard.
    pub const fn new() -> Self {
        Self {
            phase: CalPhase::Idle,
            pedal: 0,
            min: 0,
            max: ADC_FULL_SCALE,
        }
    }

    /// Begin calibrating `pedal`; the next [`advance`] captures the heel.
    ///
    /// [`advance`]: CalibrationWizard::advance
    pub fn start(&mut self, pedal: usize) {
        self.phase = CalPhase::AwaitMin;
        self.pedal = pedal;
    }

    /// Advance one step using the pedal's current raw reading. Step 1
    /// captures the minimum, step 2 the maximum, step 3 (from `Complete`)
    /// acknowledges and returns to `Idle`. Returns the new phase.
    pub fn advance(&mut self, raw: u16) -> CalPhase {
        self.phase = match self.phase {
            CalPhase::AwaitMin => {
                self.min = raw;
                CalPhase::AwaitMax
            }
            CalPhase::AwaitMax => {
                self.max = raw;
                CalPhase::Complete
            }
            CalPhase::Complete | CalPhase::Idle => CalPhase::Idle,
        };
        self.phase
    }

    /// Abort and return to `Idle`, discarding any captured endpoints.
    pub fn cancel(&mut self) {
        self.phase = CalPhase::Idle;
    }

    /// Current phase.
    pub fn phase(&self) -> CalPhase {
        self.phase
    }

    /// The pedal index currently being calibrated (`0` or `1`).
    pub fn pedal(&self) -> usize {
        self.pedal
    }

    /// The captured `(pedal, min, max)` once `Complete` *and* valid
    /// (`max > min`). Returns `None` if the endpoints were captured
    /// backwards or the pedal didn't move — the caller should restart the
    /// wizard. Mirrors `menu.py`'s validity guard.
    pub fn result(&self) -> Option<(usize, u16, u16)> {
        if matches!(self.phase, CalPhase::Complete) && self.max > self.min {
            Some((self.pedal, self.min, self.max))
        } else {
            None
        }
    }
}

impl Default for CalibrationWizard {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hardware binding + sampling task ───────────────────────────────────

/// One pedal's ADC channel paired with its signal processor.
struct Pedal {
    ch: AdcChannel<'static>,
    proc: PedalProcessor,
}

/// Owns the ADC and both pedal channels — the single owner of the
/// expression-pedal hardware. Construct once, hand to [`expression_task`].
pub struct ExpressionInputs {
    adc: Adc<'static, Async>,
    pedals: [Pedal; PEDAL_COUNT],
}

impl ExpressionInputs {
    /// Bind the (already interrupt-configured) async ADC to the two pedal
    /// pins. Pass `p.PIN_27` as `pedal0` and `p.PIN_28` as `pedal1`. The
    /// pins are analog inputs, so no pull is applied.
    pub fn new(
        adc: Adc<'static, Async>,
        pedal0: Peri<'static, impl AdcPin + 'static>,
        pedal1: Peri<'static, impl AdcPin + 'static>,
    ) -> Self {
        Self {
            adc,
            pedals: [
                Pedal {
                    ch: AdcChannel::new_pin(pedal0, Pull::None),
                    proc: PedalProcessor::new(),
                },
                Pedal {
                    ch: AdcChannel::new_pin(pedal1, Pull::None),
                    proc: PedalProcessor::new(),
                },
            ],
        }
    }

    /// Set a pedal's calibration endpoints (see
    /// [`PedalProcessor::set_calibration`]). Out-of-range `pedal` indices
    /// and invalid ranges return `false`.
    pub fn set_calibration(&mut self, pedal: usize, min: u16, max: u16) -> bool {
        match self.pedals.get_mut(pedal) {
            Some(p) => p.proc.set_calibration(min, max),
            None => false,
        }
    }

    /// Set a pedal's response curve. No-op for out-of-range indices.
    pub fn set_curve(&mut self, pedal: usize, curve: Curve) {
        if let Some(p) = self.pedals.get_mut(pedal) {
            p.proc.set_curve(curve);
        }
    }
}

/// Sample both pedals at [`SAMPLE_RATE_HZ`] forever, emitting an
/// [`ExprEvent`] each time a mapped value changes.
///
/// Drop-newest on a full channel (ARCHITECTURE.md channel rule #2): a lost
/// reading is superseded by the next one anyway, so we never block the
/// sampler.
#[embassy_executor::task]
pub async fn expression_task(mut inputs: ExpressionInputs, sender: ExprSender) {
    // Split the borrow so `adc` and `pedals` can be used disjointly inside
    // the loop (the ADC read borrows `adc`; the processor borrows a pedal).
    let ExpressionInputs { adc, pedals } = &mut inputs;

    let mut ticker = Ticker::every(Duration::from_hz(SAMPLE_RATE_HZ));
    loop {
        // Apply a calibration the menu pushed since the last tick (live, no
        // reboot). Cleared once consumed.
        if let Some(cals) = LIVE_CAL.lock(|c| c.take()) {
            for (pedal, cal) in pedals.iter_mut().zip(cals.iter()) {
                pedal.proc.set_calibration(cal.min, cal.max);
            }
        }

        let mut raws = LATEST_RAW.lock(|c| c.get());
        for (i, pedal) in pedals.iter_mut().enumerate() {
            match adc.read(&mut pedal.ch).await {
                Ok(raw) => {
                    raws[i] = raw; // publish for the calibration wizard
                    if let Some(value) = pedal.proc.push(raw) {
                        let event = ExprEvent {
                            pedal: i as u8,
                            value,
                        };
                        if sender.try_send(event).is_err() {
                            warn!("expr: channel full, dropped pedal {} = {}", i as u8, value);
                        }
                    }
                }
                Err(e) => warn!("expr: ADC read error on pedal {}: {:?}", i as u8, e),
            }
        }
        LATEST_RAW.lock(|c| c.set(raws));
        ticker.next().await;
    }
}
