//! `midi_engine_test` — proof binary for the `src/midi/` engine.
//!
//! Part 1 — **self-test** (runs at boot; needs no hardware or host).
//! Exercises the pure codec and asserts byte-exactness against vectors
//! derived from the CircuitPython reference (`remedy/lib/midi.py`):
//!
//! - Katana DT1/RQ1 builders + Roland checksum + 11-bit encoding.
//! - SysEx USB-packetise → reassemble round-trip.
//! - Channel-voice decode from a USB packet and from a DIN running-status
//!   stream.
//!
//! Every check is a `defmt::assert!`, so a flashed board either prints
//! `midi self-test: ALL PASS` over RTT or panics with the failing case.
//!
//! Part 2 — **live engine**. Wires the USB-MIDI class and DIN UART0 to the
//! mux (`src/midi/mux.rs`) and logs every parsed `MidiRx` / reassembled
//! SysEx. A small injector pushes a demo CC and a Katana gain-set SysEx
//! outbound a couple of seconds after boot, so there is traffic on both
//! transports even with nothing plugged into MIDI IN.
//!
//! Transport setup mirrors `examples/midi_passthrough.rs`.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::{join, join3};
use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{UART0, USB};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, Config as UartConfig};
use embassy_rp::usb::{Driver, InterruptHandler as UsbIrq};
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use embassy_usb::class::midi::MidiClass;
use embassy_usb::{Builder, Config as UsbConfig};
use midicaptain_firmware::events::{MidiCmd, MidiRx};
use midicaptain_firmware::midi::{katana, mux, sysex};
use midicaptain_firmware::pins;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbIrq<USB>;
    UART0_IRQ   => BufferedInterruptHandler<UART0>;
});

// ── Mux channels ───────────────────────────────────────────────────────
// Two SysEx channels: one inbound (reassembled → app), one outbound
// (app → both transports). RX/CMD carry channel-voice.
static RX: mux::MidiRxChannel = Channel::new();
static CMD: mux::MidiCmdChannel = Channel::new();
static SYSEX_IN: mux::SysExChannel = Channel::new();
static SYSEX_OUT: mux::SysExChannel = Channel::new();

type UsbDriver = Driver<'static, USB>;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("midi_engine_test: boot");

    // ── 1. Logic self-test (transport-independent) ─────────────────────
    self_test();

    let p = embassy_rp::init(Default::default());

    // ── USB-MIDI device ────────────────────────────────────────────────
    let driver = Driver::new(p.USB, Irqs);
    let mut config = UsbConfig::new(pins::USB_VID, pins::USB_PID);
    config.manufacturer = Some(pins::USB_MANUFACTURER);
    config.product = Some(pins::USB_PRODUCT);
    config.serial_number = Some("RMDY-DEV-0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    let mut builder = {
        static CFG: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS: StaticCell<[u8; 256]> = StaticCell::new();
        static CTL: StaticCell<[u8; 64]> = StaticCell::new();
        Builder::new(
            driver,
            config,
            CFG.init([0; 256]),
            BOS.init([0; 256]),
            &mut [],
            CTL.init([0; 64]),
        )
    };
    let midi = MidiClass::new(&mut builder, 1, 1, 64);
    let (usb_tx, usb_rx) = midi.split();
    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    // ── DIN MIDI over UART0 ────────────────────────────────────────────
    static TX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static RX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    let mut uart_config = UartConfig::default();
    uart_config.baudrate = pins::MIDI_BAUD;
    let uart = BufferedUart::new(
        p.UART0,
        p.PIN_16, // pins::MIDI_TX_PIN
        p.PIN_17, // pins::MIDI_RX_PIN
        Irqs,
        TX_BUF.init([0; 64]),
        RX_BUF.init([0; 64]),
        uart_config,
    );
    let (din_tx, din_rx) = uart.split();

    // ── Consumer: log everything the mux normalises inbound ────────────
    let monitor = async {
        loop {
            match select(RX.receive(), SYSEX_IN.receive()).await {
                Either::First(m) => info!("RX voice: {}", m),
                Either::Second(sx) => {
                    info!("RX sysex ({} bytes): {=[u8]:02x}", sx.len(), sx.as_slice())
                }
            }
        }
    };

    // ── Producer: demo outbound traffic after USB enumerates ───────────
    let injector = async {
        Timer::after_secs(2).await;
        let _ = CMD.try_send(MidiCmd::ControlChange {
            channel: 0,
            cc: 80,
            value: 127,
        });
        if let Ok(sx) = katana::set_gain(50) {
            let _ = SYSEX_OUT.try_send(sx);
        }
        info!("injector: sent demo CC + Katana gain SysEx to both transports");
        core::future::pending::<()>().await
    };

    // ── Run the mux + monitor + injector forever ───────────────────────
    join(
        join3(
            mux::usb_in_loop(usb_rx, &RX, &SYSEX_IN),
            mux::din_in_loop(din_rx, &RX, &SYSEX_IN),
            mux::out_loop(usb_tx, din_tx, &CMD, &SYSEX_OUT),
        ),
        join(monitor, injector),
    )
    .await;
}

#[embassy_executor::task]
async fn usb_task(mut device: embassy_usb::UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

/// Transport-independent correctness checks. Panics (over defmt) on any
/// mismatch; prints `ALL PASS` otherwise.
fn self_test() {
    // 1. Katana DT1 build is byte-exact vs the CircuitPython reference.
    //    set_gain(50): F0 41 00 00 00 00 33 12 00 00 04 21 32 29 F7
    const GAIN_50: [u8; 15] = [
        0xF0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x33, 0x12, 0x00, 0x00, 0x04, 0x21, 0x32, 0x29, 0xF7,
    ];
    let gain = katana::set_gain(50).unwrap();
    info!("katana set_gain(50) = {=[u8]:02x}", gain.as_slice());
    defmt::assert!(gain.as_slice() == &GAIN_50[..], "set_gain(50) byte mismatch");

    //    rq1 read of gain (len 1):
    //    F0 41 00 00 00 00 33 11 00 00 04 21 00 00 00 01 5A F7
    const RQ1_GAIN: [u8; 18] = [
        0xF0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x33, 0x11, 0x00, 0x00, 0x04, 0x21, 0x00, 0x00, 0x00,
        0x01, 0x5A, 0xF7,
    ];
    let rq = katana::rq1(&katana::KATANA_MODEL_ID, &[0x00, 0x00, 0x04, 0x21], 1).unwrap();
    defmt::assert!(rq.as_slice() == &RQ1_GAIN[..], "RQ1 gain-read byte mismatch");

    // Checksum + 11-bit primitives.
    defmt::assert!(
        katana::roland_checksum(&[0x00, 0x00, 0x04, 0x21, 0x32]) == 0x29,
        "roland_checksum mismatch"
    );
    let enc = katana::encode_11bit(500);
    defmt::assert!(enc == [0x03, 0x74], "encode_11bit(500) mismatch");
    defmt::assert!(katana::decode_11bit(enc[0], enc[1]) == 500, "decode_11bit mismatch");

    // 2. SysEx USB packetise → reassemble round-trips byte-for-byte.
    let delay = katana::set_delay_time(500).unwrap();
    let mut packets: heapless::Vec<[u8; 4], 96> = heapless::Vec::new();
    sysex::to_usb_packets(&delay, 0, &mut packets).unwrap();
    info!("delay-time SysEx -> {} USB packet(s)", packets.len());

    let mut reasm = sysex::SysExBuf::new();
    let mut round: sysex::SysEx = sysex::SysEx::new();
    let mut got = false;
    for p in &packets {
        if let Some(m) = reasm.push_packet(p) {
            round.clear();
            round.extend_from_slice(m).unwrap();
            got = true;
        }
    }
    defmt::assert!(got, "no SysEx reassembled from packets");
    defmt::assert!(
        round.as_slice() == delay.as_slice(),
        "SysEx USB round-trip mismatch"
    );
    info!("SysEx round-trip OK ({} bytes)", round.len());

    // 3a. Channel-voice decode from a USB packet (CC #80=64, ch 0, CIN B).
    let cc = mux::decode_usb_channel(&[0x0B, 0xB0, 80, 64]).unwrap();
    info!("decoded USB packet -> {}", cc);

    // 3b. DIN running-status: NoteOn ch0 n60 v100, then (running) n62 v0,
    //     where v0 normalises to NoteOff.
    let mut parser = mux::DinParser::new();
    let stream = [0x90u8, 60, 100, 62, 0];
    let mut notes = 0u32;
    for &b in &stream {
        if let Some(mux::DinOut::Rx(m)) = parser.feed(b) {
            info!("DIN stream -> {}", m);
            notes += 1;
        }
    }
    defmt::assert!(notes == 2, "running-status decode produced {=u32} notes", notes);

    // 3c. Pitch-bend decode (LSB first, then MSB; 14-bit). Centre packet
    //     (LSB 0x00, MSB 0x40) → 8192; all-ones → 16383.
    defmt::assert!(
        mux::decode_usb_channel(&[0x0E, 0xE0, 0x00, 0x40])
            == Some(MidiRx::PitchBend { channel: 0, value: 8192 }),
        "pitch-bend centre decode failed"
    );
    defmt::assert!(
        mux::decode_usb_channel(&[0x0E, 0xE0, 0x7F, 0x7F])
            == Some(MidiRx::PitchBend { channel: 0, value: 16383 }),
        "pitch-bend max decode failed"
    );

    info!("midi self-test: ALL PASS");
}
