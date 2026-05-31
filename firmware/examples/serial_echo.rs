//! `serial_echo` — minimum-viable USB CDC ACM endpoint that echoes bytes
//! and honours the 1200-baud BOOTSEL touch convention.
//!
//! Why this matters:
//! - The eventual webapp ↔ device sync protocol rides on top of CDC. This
//!   binary proves the transport works against host tooling (PowerShell
//!   `miniterm`, PuTTY, the `serial` crate, etc.).
//! - `scripts/bootsel_hammer.py` opens this port at 1200 baud and drops
//!   DTR to force the device into the RP2040 mass-storage bootloader.
//!   That recovery channel only works if WE detect the magic baud rate
//!   and call `reset_to_usb_boot`. We do.
//!
//! USB identity: `pins::USB_VID` / `pins::USB_PID` (Raspberry Pi VID,
//! development PID). The product string says "MIDICaptain Remedy (Rust)"
//! so `bootsel_hammer.py`'s keyword match hits even on hosts that hide
//! the VID.

#![no_std]
#![no_main]

use defmt::{info, panic, warn};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::rom_data::reset_to_usb_boot;
use embassy_rp::usb::{Driver, InterruptHandler as UsbIrq};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::cdc_acm::{CdcAcmClass, ControlChanged, Sender, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, Config};
use midicaptain_firmware::pins;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbIrq<USB>;
});

/// The magic baud rate that signals "reboot me into the bootloader".
/// Convention is shared across Arduino, Pico SDK, TinyUSB, etc.
const BOOTSEL_BAUD: u32 = 1200;

/// Pipe RX → TX without forcing them onto the same task. Capacity is
/// generous because the RX side runs in lockstep with the TX side; in
/// practice the channel never holds more than one entry.
type EchoChan = Channel<CriticalSectionRawMutex, EchoFrame, 4>;
static ECHO: EchoChan = Channel::new();

#[derive(Copy, Clone)]
struct EchoFrame {
    bytes: [u8; 64],
    len:   u8,
}

type UsbDriver = Driver<'static, USB>;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("MIDICaptain serial_echo: boot");
    let p = embassy_rp::init(Default::default());
    let driver = Driver::new(p.USB, Irqs);

    // ── USB device descriptors ────────────────────────────────────────
    let mut config = Config::new(pins::USB_VID, pins::USB_PID);
    config.manufacturer  = Some(pins::USB_MANUFACTURER);
    config.product       = Some(pins::USB_PRODUCT);
    config.serial_number = Some("RMDY-DEV-0001");
    config.max_power     = 100; // mA
    config.max_packet_size_0 = 64;

    // CDC needs IAD (Interface Association Descriptor) so Windows can
    // associate the comm + data interfaces. Becomes essential the moment
    // we add MIDI alongside CDC in the same composite device.
    config.composite_with_iads = true;
    config.device_class     = 0xEF; // Misc
    config.device_sub_class = 0x02;
    config.device_protocol  = 0x01;

    // Descriptor scratch buffers must outlive the builder.
    let mut builder = {
        static CFG: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS: StaticCell<[u8; 256]> = StaticCell::new();
        static CTL: StaticCell<[u8; 64]>  = StaticCell::new();
        Builder::new(
            driver,
            config,
            CFG.init([0; 256]),
            BOS.init([0; 256]),
            &mut [], // no MS-OS descriptors
            CTL.init([0; 64]),
        )
    };

    // ── CDC ACM class ─────────────────────────────────────────────────
    static STATE: StaticCell<State> = StaticCell::new();
    let class = CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let (tx, mut rx, control) = class.split_with_control();

    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    // RX task: read packets and shove them at the writer/watcher.
    let read_fut = async {
        let mut buf = [0u8; 64];
        loop {
            rx.wait_connection().await;
            info!("CDC connected");
            loop {
                match rx.read_packet(&mut buf).await {
                    Ok(n) => {
                        let mut bytes = [0u8; 64];
                        bytes[..n].copy_from_slice(&buf[..n]);
                        let frame = EchoFrame { bytes, len: n as u8 };
                        // Drop-newest if congested. For echo, the only
                        // way to fill 4 slots is the writer task hanging.
                        let _ = ECHO.try_send(frame);
                    }
                    Err(EndpointError::Disabled) => {
                        info!("CDC disconnected");
                        break;
                    }
                    Err(EndpointError::BufferOverflow) => {
                        panic!("CDC RX buffer overflow");
                    }
                }
            }
        }
    };

    // TX-and-watcher task: writes echo frames, AND watches for the
    // 1200-baud BOOTSEL touch. We co-locate them because we need the
    // `Sender::line_coding()` accessor (ControlChanged in embassy-usb
    // 0.6.0 doesn't expose line coding — only DTR/RTS). When that lands
    // upstream we can split this out cleanly.
    let write_fut = writer_and_watcher(tx, control);

    join(read_fut, write_fut).await;
}

#[embassy_executor::task]
async fn usb_task(mut device: embassy_usb::UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

/// Drain the echo channel onto the TX endpoint, racing against control
/// changes. On a 1200-baud + DTR-drop combo, reset to BOOTSEL.
async fn writer_and_watcher(
    mut tx: Sender<'static, UsbDriver>,
    control: ControlChanged<'static>,
) {
    loop {
        match select(ECHO.receive(), control.control_changed()).await {
            Either::First(frame) => {
                let data = &frame.bytes[..frame.len as usize];
                if let Err(e) = tx.write_packet(data).await {
                    warn!("write err: {:?}", e);
                }
            }
            Either::Second(()) => {
                let baud = tx.line_coding().data_rate();
                let dtr  = control.dtr();
                defmt::debug!("ctrl change: baud={}, dtr={}", baud, dtr);
                if baud == BOOTSEL_BAUD && !dtr {
                    info!("1200-baud touch detected — rebooting to BOOTSEL");
                    // (disable_iface_mask, gpio_activity_pin_mask).
                    // 0/0 = default: USB MSC + PICOBOOT enabled, no
                    // activity LED.
                    reset_to_usb_boot(0, 0);
                }
            }
        }
    }
}
