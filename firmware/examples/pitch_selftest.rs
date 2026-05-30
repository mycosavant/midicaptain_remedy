//! `pitch_selftest` — proof binary for the YIN pitch detector (`src/pitch.rs`).
//!
//! Needs **no hardware and no analog front-end**: it synthesises tones in
//! software (a numerically-controlled oscillator over a sine LUT), runs them
//! through [`PitchDetector`], and `defmt::assert!`s the detected note + cents.
//! A flashed board prints `pitch self-test: ALL PASS` over RTT or panics on the
//! first failing case — so the DSP math is verified on real silicon long
//! before the audio input circuit exists.
//!
//! Two layers:
//! 1. `freq_to_note_cents` checked directly at exact frequencies (the note/
//!    cents mapping, independent of YIN).
//! 2. Full detection on pure tones across the guitar/bass range, plus a
//!    harmonic-rich tone (fundamental + 2nd + 3rd) to prove YIN picks the
//!    fundamental and not an octave.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use midicaptain_firmware::pitch::{freq_to_note_cents, PitchDetector};
use {defmt_rtt as _, panic_probe as _};

/// Sample rate the detector + the synthesised tones agree on.
const FS: u32 = 16_000;
/// Analysis window length (samples). 1024 @ 16 kHz = 64 ms — several periods
/// even at low E (82 Hz).
const N: usize = 1024;

/// One period of a sine, 256 points, amplitude 1500 (within a 12-bit ADC's
/// ±2048 swing). Generated offline.
#[rustfmt::skip]
const SINE256: [i16; 256] = [
    0, 37, 74, 110, 147, 184, 220, 256, 293, 329, 364, 400, 435, 471, 505, 540,
    574, 608, 641, 674, 707, 739, 771, 802, 833, 864, 894, 923, 952, 980, 1007, 1034,
    1061, 1086, 1111, 1136, 1160, 1183, 1205, 1226, 1247, 1267, 1287, 1305, 1323, 1340, 1356, 1371,
    1386, 1399, 1412, 1424, 1435, 1446, 1455, 1464, 1471, 1478, 1484, 1489, 1493, 1496, 1498, 1500,
    1500, 1500, 1498, 1496, 1493, 1489, 1484, 1478, 1471, 1464, 1455, 1446, 1435, 1424, 1412, 1399,
    1386, 1371, 1356, 1340, 1323, 1305, 1287, 1267, 1247, 1226, 1205, 1183, 1160, 1136, 1111, 1086,
    1061, 1034, 1007, 980, 952, 923, 894, 864, 833, 802, 771, 739, 707, 674, 641, 608,
    574, 540, 505, 471, 435, 400, 364, 329, 293, 256, 220, 184, 147, 110, 74, 37,
    0, -37, -74, -110, -147, -184, -220, -256, -293, -329, -364, -400, -435, -471, -505, -540,
    -574, -608, -641, -674, -707, -739, -771, -802, -833, -864, -894, -923, -952, -980, -1007, -1034,
    -1061, -1086, -1111, -1136, -1160, -1183, -1205, -1226, -1247, -1267, -1287, -1305, -1323, -1340, -1356, -1371,
    -1386, -1399, -1412, -1424, -1435, -1446, -1455, -1464, -1471, -1478, -1484, -1489, -1493, -1496, -1498, -1500,
    -1500, -1500, -1498, -1496, -1493, -1489, -1484, -1478, -1471, -1464, -1455, -1446, -1435, -1424, -1412, -1399,
    -1386, -1371, -1356, -1340, -1323, -1305, -1287, -1267, -1247, -1226, -1205, -1183, -1160, -1136, -1111, -1086,
    -1061, -1034, -1007, -980, -952, -923, -894, -864, -833, -802, -771, -739, -707, -674, -641, -608,
    -574, -540, -505, -471, -435, -400, -364, -329, -293, -256, -220, -184, -147, -110, -74, -37,
];

/// Fill `buf` with a sum of sinusoids. Each `(inc, shift)` is an NCO phase
/// increment (`f/FS · 2^32`) and a right-shift attenuation, so harmonics can
/// be mixed in. Up to 4 partials.
fn synth(buf: &mut [i16], partials: &[(u32, u8)]) {
    let mut phase = [0u32; 4];
    for s in buf.iter_mut() {
        let mut acc: i32 = 0;
        for (k, &(inc, shift)) in partials.iter().enumerate() {
            phase[k] = phase[k].wrapping_add(inc);
            acc += (SINE256[(phase[k] >> 24) as usize & 0xFF] as i32) >> shift;
        }
        *s = acc as i16;
    }
}

fn run_tests() {
    // ── 1. note/cents mapping at exact frequencies (milli-Hz) ──────────
    // (freq, expected_note, expected_cents). Cents within ±1 of the ideal.
    let map_cases: [(u32, u8, i16); 6] = [
        (440_000, 69, 0),   // A4
        (445_100, 69, 20),  // A4 +20c
        (82_410, 40, 0),    // E2
        (81_700, 40, -15),  // E2 −15c
        (261_630, 60, 0),   // C4
        (329_630, 64, 0),   // E4
    ];
    for (f, en, ec) in map_cases {
        let (note, cents) = freq_to_note_cents(f);
        defmt::assert!(
            note == en && (cents - ec).abs() <= 1,
            "map {=u32}mHz -> note {=u8} cents {=i16}, want note {=u8} cents {=i16}",
            f, note, cents, en, ec
        );
    }
    info!("pitch self-test: note/cents map OK");

    // ── 2. full detection on synthesised tones ─────────────────────────
    let det = PitchDetector::new(FS);
    let mut diff = [0i64; 320]; // > max_tau (FS/60 = 266)
    let mut buf = [0i16; N];

    // (name, partials, expected_note, expected_cents, cents_tol).
    // Phase increments are f/FS·2^32 for FS = 16 kHz.
    type ToneCase = (&'static str, &'static [(u32, u8)], u8, i16, i16);
    let tone_cases: [ToneCase; 9] = [
        ("E2 82.41",  &[(0x0151_8D26, 0)],                                  40,  0, 3),
        ("A2 110",    &[(0x01C2_8F5C, 0)],                                  45,  0, 3),
        ("D3 146.83", &[(0x0259_6A6A, 0)],                                  50,  0, 3),
        ("G3 196",    &[(0x0322_D0E5, 0)],                                  55,  0, 3),
        ("B3 246.94", &[(0x03F3_775C, 0)],                                  59,  0, 3),
        ("C4 261.63", &[(0x042F_A2F0, 0)],                                  60,  0, 3),
        ("E4 329.63", &[(0x0546_2A1B, 0)],                                  64,  0, 3),
        ("A4+20c",    &[(0x071F_212D, 0)],                                  69, 20, 4),
        // E2 with 2nd + 3rd harmonics — must resolve to the fundamental.
        ("E2 harm",   &[(0x0151_8D26, 0), (0x02A3_1A4C, 1), (0x03F4_A772, 2)], 40, 0, 5),
    ];
    for (name, partials, en, ec, tol) in tone_cases {
        synth(&mut buf, partials);
        let p = defmt::unwrap!(det.detect(&buf, &mut diff), "{=str}: no pitch detected", name);
        info!(
            "  {=str}: f={=u32}mHz note={=u8} cents={=i16}",
            name, p.freq_milli_hz, p.note, p.cents
        );
        defmt::assert!(
            p.note == en && (p.cents - ec).abs() <= tol,
            "{=str}: got note {=u8} cents {=i16}, want note {=u8} cents {=i16} (±{=i16})",
            name, p.note, p.cents, en, ec, tol
        );
    }

    // ── 3. silence rejects ─────────────────────────────────────────────
    for s in buf.iter_mut() {
        *s = 0;
    }
    defmt::assert!(det.detect(&buf, &mut diff).is_none(), "silence should not detect");

    info!("pitch self-test: ALL PASS");
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("pitch_selftest: boot");
    // Bring up clocks so the time driver / heartbeat works (no peripherals used).
    let _p = embassy_rp::init(Default::default());

    run_tests();

    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("pitch self-test: ALL PASS (idle)");
    }
}
