//! Monophonic pitch detection for the on-device tuner.
//!
//! The MIDI Captain has no audio input on the stock board — but it has a spare
//! ADC channel (`ADC0`/GP26), so with a small analog front-end (see
//! `HARDWARE.md`) it can sample the guitar/line signal and detect pitch
//! itself, making the tuner truly standalone (no host software, unlike the
//! HKAudio firmware which only *displays* tuning data pushed to it over MIDI).
//!
//! Algorithm: **YIN** (de Cheveigné & Kawahara, 2002) — the autocorrelation
//! family's robust cousin, chosen because it resists the octave errors plain
//! autocorrelation makes on a harmonically-rich guitar tone. It runs entirely
//! in integer arithmetic (the RP2040's Cortex-M0+ has no FPU and no hardware
//! divide), band-limited to the guitar/bass fundamental range so the
//! difference function stays cheap (~`MAX_TAU × window` ≈ 200k MACs/frame,
//! a few ms at 125 MHz — far faster than the ~16 ms it takes to *collect* a
//! frame, so detection keeps up with the input).
//!
//! This module is pure computation: no embassy, no peripherals. It takes a
//! slice of samples and returns a [`Pitch`], which makes it directly testable
//! on-device (`examples/pitch_selftest.rs` feeds it synthesised tones).

/// Lowest fundamental tracked (Hz). ~B1 covers 5-string bass / drop tunings.
const MIN_FREQ_HZ: u32 = 60;
/// Highest fundamental tracked (Hz). Above the 24th-fret high E (~1319 Hz).
const MAX_FREQ_HZ: u32 = 1400;

/// YIN absolute threshold as a fraction `NUM/DEN` (0.10). A `tau` whose
/// cumulative-mean-normalised difference dips below this counts as a confident
/// period candidate.
const THRESHOLD_NUM: i64 = 10;
const THRESHOLD_DEN: i64 = 100;

/// Minimum per-sample RMS (in ADC LSBs) for a frame to be considered "signal"
/// rather than noise/silence. Below this, [`PitchDetector::detect`] returns
/// `None` so the display reads `--` instead of chasing the noise floor.
const MIN_RMS: i64 = 24;

/// `log2(1 + i/32)` in Q16, `i = 0..=32`. Used by [`freq_to_note_cents`] for
/// the only transcendental the tuner needs; interpolated between entries.
const LOG2_LUT: [i32; 33] = [
    0, 2909, 5732, 8473, 11136, 13727, 16248, 18704, 21098, 23433, 25711, 27936, 30109, 32234,
    34312, 36346, 38336, 40286, 42196, 44068, 45904, 47705, 49472, 51207, 52911, 54584, 56229,
    57845, 59434, 60997, 62534, 64047, 65536,
];

/// A4 reference pitch in milli-Hz (440.000 Hz).
const A4_MILLI_HZ: u32 = 440_000;

/// A detected pitch.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Pitch {
    /// Estimated fundamental, milli-Hz (sub-sample interpolated).
    pub freq_milli_hz: u32,
    /// Nearest MIDI note number (`69` = A4).
    pub note: u8,
    /// Deviation from `note`, in cents (`-50..=50`).
    pub cents: i16,
}

/// Stateless YIN pitch detector, fixed to one sample rate.
pub struct PitchDetector {
    sample_rate: u32,
}

impl PitchDetector {
    pub const fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }

    /// Longest lag the detector needs, for sizing the `diff` scratch buffer:
    /// `diff.len()` passed to [`Self::detect`] must be `>= max_tau() + 1`.
    pub const fn max_tau(&self) -> usize {
        (self.sample_rate / MIN_FREQ_HZ) as usize
    }

    /// Detect the fundamental in `samples`. `diff` is caller-owned scratch for
    /// the YIN difference function (length `>= max_tau()+1`; contents
    /// ignored on entry). Returns `None` for silence, noise, or no confident
    /// period (e.g. a chord). DC is removed internally.
    pub fn detect(&self, samples: &[i16], diff: &mut [i64]) -> Option<Pitch> {
        let n = samples.len();
        let max_tau = self.max_tau();
        let min_tau = (self.sample_rate / MAX_FREQ_HZ) as usize;
        // Need a non-trivial integration window beyond the longest lag.
        if max_tau < min_tau + 2 || n <= max_tau + min_tau || diff.len() <= max_tau {
            return None;
        }
        let w = n - max_tau; // integration window length

        // ── DC removal + energy gate ───────────────────────────────────
        let mut sum: i64 = 0;
        for &x in samples {
            sum += x as i64;
        }
        let mean = (sum / n as i64) as i32;
        let mut energy: i64 = 0;
        for &x in samples {
            let c = (x as i32 - mean) as i64;
            energy += c * c;
        }
        if energy < MIN_RMS * MIN_RMS * n as i64 {
            return None; // below the noise floor
        }

        // ── YIN difference function d[tau], tau = 1..=max_tau ──────────
        diff[0] = 0;
        for tau in 1..=max_tau {
            let mut acc: i64 = 0;
            for j in 0..w {
                let a = samples[j] as i32 - mean;
                let b = samples[j + tau] as i32 - mean;
                let d = (a - b) as i64;
                acc += d * d;
            }
            diff[tau] = acc;
        }

        // ── Cumulative-mean-normalised threshold search ────────────────
        // d'[tau] < T  ⇔  DEN·d[tau]·tau < NUM·Σ_{k=1}^{tau} d[k]  (no divide).
        // On the first dip below T, descend to its local minimum.
        let mut cumulative: i64 = 0;
        let mut tau0: usize = 0;
        for tau in 1..=max_tau {
            cumulative += diff[tau];
            if tau < min_tau {
                continue;
            }
            if THRESHOLD_DEN * diff[tau] * (tau as i64) < THRESHOLD_NUM * cumulative {
                let mut t = tau;
                while t < max_tau && diff[t + 1] < diff[t] {
                    t += 1;
                }
                tau0 = t;
                break;
            }
        }
        if tau0 == 0 {
            return None; // no period crossed the confidence threshold
        }

        // ── Parabolic interpolation for sub-sample period (Q16) ────────
        let tau_q16 = if tau0 > min_tau && tau0 < max_tau {
            let dm = diff[tau0 - 1];
            let d0 = diff[tau0];
            let dp = diff[tau0 + 1];
            let den = 2 * (dm - 2 * d0 + dp);
            if den > 0 {
                let delta_q16 = ((dm - dp) << 16) / den; // ∈ (-65536, 65536)
                ((tau0 as i64) << 16) + delta_q16
            } else {
                (tau0 as i64) << 16
            }
        } else {
            (tau0 as i64) << 16
        };

        // f0 = Fs / tau. tau is Q16, so f0_mHz = Fs·1000·2^16 / tau_q16.
        let freq_milli_hz = (((self.sample_rate as i64 * 1000) << 16) / tau_q16) as u32;
        let (note, cents) = freq_to_note_cents(freq_milli_hz);
        Some(Pitch {
            freq_milli_hz,
            note,
            cents,
        })
    }
}

/// Map a fundamental (milli-Hz) to the nearest MIDI note and cents deviation.
/// `note_float = 69 + 12·log2(f / 440)`; the rounded value is the note and the
/// remainder (×100) the cents, clamped to `±50`.
pub fn freq_to_note_cents(freq_milli_hz: u32) -> (u8, i16) {
    let f = freq_milli_hz.max(1);
    // 12·(log2 f − log2 440) in Q16.
    let semis_q16 = 12 * (log2_q16(f) - log2_q16(A4_MILLI_HZ));
    let note_q16 = (69i32 << 16) + semis_q16;
    let note = round_q16(note_q16).clamp(0, 127);
    let cents = round_q16((note_q16 - (note << 16)) * 100).clamp(-50, 50);
    (note as u8, cents as i16)
}

/// `log2(x)` in Q16 for `x >= 1`. Integer part from the leading bit; fractional
/// part from [`LOG2_LUT`] with linear interpolation.
fn log2_q16(x: u32) -> i32 {
    let x = x.max(1);
    let e = 31 - x.leading_zeros(); // floor(log2 x), 0..=31
    // Normalise so the leading 1 sits at bit 30: m ∈ [2^30, 2^31).
    let m = if e >= 30 { x >> (e - 30) } else { x << (30 - e) };
    let frac = m - (1u32 << 30); // ∈ [0, 2^30): fractional mantissa
    let idx = (frac >> 25) as usize; // top 5 bits → 0..=31
    let sub = ((frac >> 9) & 0xFFFF) as i64; // next 16 bits → Q16 within step
    let lo = LOG2_LUT[idx] as i64;
    let hi = LOG2_LUT[idx + 1] as i64;
    let interp = lo + ((hi - lo) * sub) / 65536;
    (e as i64 * 65536 + interp) as i32
}

/// Round a Q16 fixed-point value to the nearest integer (half away from zero).
fn round_q16(v: i32) -> i32 {
    if v >= 0 {
        (v + 0x8000) >> 16
    } else {
        -((-v + 0x8000) >> 16)
    }
}
