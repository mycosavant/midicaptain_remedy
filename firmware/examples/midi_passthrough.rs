//! `midi_passthrough` — bridge between USB-MIDI and the 5-pin DIN MIDI
//! port. Bytes arriving on USB get forwarded to UART0 and vice versa.
//!
//! Proves both MIDI transports are operational end-to-end:
//! - USB-MIDI device (class-compliant; appears in DAWs as "MIDICaptain
//!   Remedy (Rust)").
//! - DIN-MIDI over UART0 at 31250 baud (GP16 TX → DIN-OUT, GP17 RX ←
//!   DIN-IN through the standard 220 Ω current-loop / opto-isolator
//!   circuit on the OEM board).
//!
//! USB-MIDI's wire format is "USB-MIDI Event Packets" (4 bytes each: a
//! cable+code-index byte followed by up to 3 MIDI status/data bytes).
//! DIN is raw MIDI. We do enough translation here to make the bridge
//! work; a real implementation will parse running status, SysEx
//! continuations, etc. — that's the next session.

#![no_std]
#![no_main]

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{UART0, USB};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, BufferedUartRx, BufferedUartTx, Config as UartConfig};
use embassy_rp::usb::{Driver, InterruptHandler as UsbIrq};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::midi::MidiClass;
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, Config as UsbConfig};
use embedded_io_async::{Read, Write};
use midicaptain_firmware::pins;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbIrq<USB>;
    UART0_IRQ   => BufferedInterruptHandler<UART0>;
});

/// Max MIDI message length we forward without splitting (NoteOn, CC, PC,
/// etc. are 1–3 bytes; SysEx can be arbitrarily long but we cap it here
/// for the PoC).
const MAX_MIDI_MSG: usize = 3;

/// Channel capacity (number of pending MIDI events in each direction).
/// 16 covers a fast burst from a controller without blocking the
/// producer; tune up if you start seeing drops.
const QUEUE_DEPTH: usize = 16;

/// One direction's queue: a small bounded MPSC of fixed-size events.
type MidiQueue = Channel<CriticalSectionRawMutex, MidiEvent, QUEUE_DEPTH>;

/// A parsed MIDI message, ready to be re-encoded for either transport.
#[derive(Copy, Clone, Debug, defmt::Format)]
struct MidiEvent {
    bytes: [u8; MAX_MIDI_MSG],
    len:   u8,
}

static USB_TO_DIN: MidiQueue = Channel::new();
static DIN_TO_USB: MidiQueue = Channel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("MIDICaptain midi_passthrough: boot");
    let p = embassy_rp::init(Default::default());

    // ── USB-MIDI device ────────────────────────────────────────────────
    let driver = Driver::new(p.USB, Irqs);
    let mut config = UsbConfig::new(pins::USB_VID, pins::USB_PID);
    config.manufacturer  = Some(pins::USB_MANUFACTURER);
    config.product       = Some(pins::USB_PRODUCT);
    config.serial_number = Some("RMDY-DEV-0001");
    config.max_power     = 100;
    config.max_packet_size_0 = 64;

    let mut builder = {
        static CFG: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS: StaticCell<[u8; 256]> = StaticCell::new();
        static CTL: StaticCell<[u8; 64]>  = StaticCell::new();
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
    let (mut usb_tx, mut usb_rx) = midi.split();
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
    spawner.spawn(din_reader(din_rx).unwrap());
    spawner.spawn(din_writer(din_tx).unwrap());

    // ── USB → DIN forwarder ────────────────────────────────────────────
    let usb_rx_fut = async {
        let mut buf = [0u8; 64];
        loop {
            usb_rx.wait_connection().await;
            info!("USB-MIDI connected");
            loop {
                match usb_rx.read_packet(&mut buf).await {
                    Ok(n) => decode_usb_midi(&buf[..n]).await,
                    Err(EndpointError::Disabled) => {
                        info!("USB-MIDI disconnected");
                        break;
                    }
                    Err(e) => warn!("USB-MIDI rx err: {:?}", e),
                }
            }
        }
    };

    // ── DIN → USB forwarder ────────────────────────────────────────────
    let usb_tx_fut = async {
        loop {
            let evt = DIN_TO_USB.receive().await;
            // Re-encode as a single USB-MIDI Event Packet. Code Index
            // Number (CIN) for a standard MIDI message can be derived
            // from the status byte's upper nibble.
            let status = evt.bytes[0];
            let cin = match status & 0xF0 {
                0x80 => 0x8, // Note Off
                0x90 => 0x9, // Note On
                0xA0 => 0xA, // Poly Aftertouch
                0xB0 => 0xB, // CC
                0xC0 => 0xC, // PC      (2-byte)
                0xD0 => 0xD, // Channel Aftertouch (2-byte)
                0xE0 => 0xE, // Pitch Bend
                _    => continue, // SysEx/realtime: skip for the PoC
            };
            let packet = [
                cin, // cable 0, code-index = cin
                evt.bytes[0],
                evt.bytes[1],
                evt.bytes[2],
            ];
            if let Err(e) = usb_tx.write_packet(&packet).await {
                warn!("USB-MIDI tx err: {:?}", e);
            }
        }
    };

    join3(usb_rx_fut, usb_tx_fut, core::future::pending::<()>()).await;
}

type UsbDriver = Driver<'static, USB>;

#[embassy_executor::task]
async fn usb_task(mut device: embassy_usb::UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

/// Read raw MIDI bytes off DIN and queue parsed events toward USB.
///
/// PoC parser: handles channel-voice messages only (status byte 0x80–0xEF).
/// No running-status, no SysEx, no realtime bytes. The full parser lives
/// in the next session.
#[embassy_executor::task]
async fn din_reader(mut rx: BufferedUartRx) -> ! {
    let mut buf = [0u8; 16];
    let mut accum: [u8; MAX_MIDI_MSG] = [0; MAX_MIDI_MSG];
    let mut accum_len: usize = 0;
    let mut expected_len: usize = 0;
    loop {
        let n = match rx.read(&mut buf).await {
            Ok(n) if n > 0 => n,
            Ok(_) => continue,
            Err(e) => {
                warn!("DIN read err: {:?}", e);
                continue;
            }
        };
        for &b in &buf[..n] {
            if b & 0x80 != 0 {
                // Status byte. Reset accumulator.
                accum[0] = b;
                accum_len = 1;
                expected_len = match b & 0xF0 {
                    0xC0 | 0xD0 => 2,
                    0x80..=0xEF => 3,
                    _ => 0, // realtime/SysEx — skip
                };
                continue;
            }
            if expected_len == 0 || accum_len == 0 {
                continue;
            }
            accum[accum_len] = b;
            accum_len += 1;
            if accum_len == expected_len {
                let evt = MidiEvent { bytes: accum, len: accum_len as u8 };
                // Drop-newest if the channel is full — better than
                // blocking the reader and back-pressuring UART RX.
                let _ = DIN_TO_USB.try_send(evt);
                accum_len = 0;
                expected_len = 0;
            }
        }
    }
}

/// Pull events queued from USB and clock them out the DIN UART.
#[embassy_executor::task]
async fn din_writer(mut tx: BufferedUartTx) -> ! {
    loop {
        let evt = USB_TO_DIN.receive().await;
        let len = evt.len as usize;
        if let Err(e) = tx.write_all(&evt.bytes[..len]).await {
            warn!("DIN write err: {:?}", e);
        }
    }
}

/// Decode a USB-MIDI Event Packet stream and enqueue events for DIN.
///
/// Each packet is exactly 4 bytes: header (cable<<4 | CIN) followed by
/// 1–3 MIDI bytes (zero-padded). The CIN tells us how many bytes are
/// significant; we infer the same from the status byte for the PoC.
async fn decode_usb_midi(data: &[u8]) {
    for packet in data.chunks_exact(4) {
        let cin = packet[0] & 0x0F;
        let len = match cin {
            0x5 | 0xF      => 1, // single byte
            0x2 | 0x6 | 0xC | 0xD => 2,
            0x3 | 0x4 | 0x7 | 0x8 | 0x9 | 0xA | 0xB | 0xE => 3,
            _ => continue,
        };
        let mut bytes = [0u8; MAX_MIDI_MSG];
        bytes[..len].copy_from_slice(&packet[1..1 + len]);
        let _ = USB_TO_DIN.try_send(MidiEvent { bytes, len: len as u8 });
    }
}
