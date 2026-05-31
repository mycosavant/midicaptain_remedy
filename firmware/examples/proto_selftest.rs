//! `proto_selftest` — proof binary for the config-sync wire codec
//! (`src/proto.rs`). No USB, no hardware beyond the board: it round-trips
//! frames through `encode`/`decode` and `defmt::assert!`s the results, so the
//! framing/CRC/COBS logic is verified before the CDC transport (B-2) layers on.
//!
//! Covers: the CRC-16 check value, round-trips for empty / small / zero-heavy
//! / >254-run / max-size payloads, the COBS "no interior 0x00" invariant, and
//! corruption detection.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use midicaptain_firmware::proto::{self, cmd, FrameError};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// Encode `(cmd, seq, payload)`, check the COBS invariants, decode it back, and
/// assert it matches.
fn roundtrip(opcode: u8, seq: u8, payload: &[u8], body: &mut [u8], out: &mut [u8], dbody: &mut [u8]) {
    let n = defmt::unwrap!(proto::encode(opcode, seq, payload, body, out));
    defmt::assert!(out[n - 1] == 0, "frame must end with the 0x00 delimiter");
    for &b in &out[..n - 1] {
        defmt::assert!(b != 0, "COBS body must contain no interior 0x00");
    }
    let decoded = defmt::unwrap!(proto::decode(&out[..n - 1], dbody));
    defmt::assert!(decoded.cmd == opcode, "cmd mismatch");
    defmt::assert!(decoded.seq == seq, "seq mismatch");
    defmt::assert!(decoded.payload == payload, "payload mismatch");
}

fn run_tests(body: &mut [u8], out: &mut [u8], dbody: &mut [u8]) {
    // 1. CRC-16/CCITT-FALSE check value.
    defmt::assert!(proto::crc16(b"123456789") == 0x29B1, "CRC16 check value failed");
    info!("proto self-test: CRC16 OK");

    // 2. round-trips across payload shapes.
    roundtrip(cmd::HELLO, 0, &[proto::PROTO_VERSION], body, out, dbody);
    roundtrip(cmd::GET_CONFIG, 1, &[], body, out, dbody);
    roundtrip(cmd::SET_CONFIG, 2, b"hello world", body, out, dbody);
    roundtrip(cmd::SET_CONFIG, 3, &[0u8; 300], body, out, dbody); // zero-heavy
    // A payload with a >254 non-zero run and scattered zeros (exercises the
    // COBS 0xFF code path and at the maximum size).
    let mut big = [0u8; proto::MAX_PAYLOAD];
    for (i, b) in big.iter_mut().enumerate() {
        *b = if i % 257 == 0 { 0 } else { (i & 0xFF) as u8 };
    }
    roundtrip(cmd::SET_CONFIG, 4, &big, body, out, dbody);
    info!("proto self-test: round-trip OK ({} byte max payload)", proto::MAX_PAYLOAD);

    // 3. corruption is rejected (CRC or COBS).
    let n = defmt::unwrap!(proto::encode(cmd::SET_CONFIG, 5, b"corrupt me", body, out));
    let frame_len = n - 1; // exclude the delimiter
    out[frame_len / 2] ^= 0xFF;
    match proto::decode(&out[..frame_len], dbody) {
        Err(FrameError::BadCrc) | Err(FrameError::BadCobs) => {}
        _ => defmt::panic!("corruption should fail decode with BadCrc/BadCobs"),
    }
    info!("proto self-test: corruption detected OK");

    info!("proto self-test: ALL PASS");
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("proto_selftest: boot");
    let _p = embassy_rp::init(Default::default());

    static BODY: StaticCell<[u8; proto::MAX_BODY]> = StaticCell::new();
    static OUT: StaticCell<[u8; proto::MAX_FRAME_LEN]> = StaticCell::new();
    static DBODY: StaticCell<[u8; proto::MAX_BODY]> = StaticCell::new();
    let body = BODY.init([0; proto::MAX_BODY]);
    let out = OUT.init([0; proto::MAX_FRAME_LEN]);
    let dbody = DBODY.init([0; proto::MAX_BODY]);

    run_tests(body, out, dbody);

    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("proto self-test: ALL PASS (idle)");
    }
}
