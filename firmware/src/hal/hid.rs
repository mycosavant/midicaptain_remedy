//! `hid` — the USB-HID keyboard + consumer-control task.
//!
//! Sole owner of the HID interrupt-IN endpoint on the composite USB device
//! (alongside USB-MIDI and the CDC config link). It consumes [`HidReport`]s off
//! a bounded channel and writes the corresponding USB HID reports to the host.
//!
//! ## What the host sees
//!
//! The device enumerates **one** HID interface exposing two report collections,
//! distinguished by Report ID (the standard way to carry more than one report
//! on a single interface — see [`REPORT_DESCRIPTOR`]):
//!
//! - **Keyboard** (Report ID 1): an 8-byte boot-keyboard report — a modifier
//!   bitmask, a reserved byte, and up to six keycodes. We only ever set the
//!   modifiers + one keycode.
//! - **Consumer control** (Report ID 3): a single 16-bit usage (media /
//!   transport keys — Play/Pause, Volume, Next/Prev …). These are *not*
//!   keyboard keys; the host routes them through this separate report.
//!
//! Keeping the descriptor lean matters on the RP2040 (over-long composite HID
//! descriptors have been reported to fail to enumerate); this one is ~65 bytes.
//!
//! ## Tap semantics
//!
//! Each [`HidReport`] is emitted as a **press then a release** ("tap"), with a
//! short [`TAP`] gap so hosts register the keystroke as a discrete event. This
//! matches the OEM SuperMode default (`send` = press + release) and is what a
//! footswitch wants: momentary, self-releasing. (Hold-to-repeat / latched HID
//! could be added later as a config knob — the wire model already distinguishes
//! the two report kinds.)
//!
//! The interface is **always present**, independent of the loaded config: a HID
//! action can appear in any pushed config, and a stable interface count keeps
//! the host from re-running driver setup each time the config changes.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Timer};
use embassy_usb::class::hid::HidWriter;
use embassy_usb::driver::Driver;

use crate::events::HidReport;

/// Channel depth for outbound HID reports. A footswitch can't generate reports
/// faster than the task drains them (each is a press + a [`TAP`] gap + a
/// release), and HID events are discrete (not idempotent like an LED frame), so
/// a few slots of headroom is right; a full queue drops the newest tap.
pub const HID_Q: usize = 8;

/// The HID report channel type. Declare one as a `static` in the binary:
/// `static HID_CH: hid::HidChannel = hid::HidChannel::new();`
pub type HidChannel = Channel<CriticalSectionRawMutex, HidReport, HID_Q>;

/// Producer handle — held by the event router.
pub type HidSender = Sender<'static, CriticalSectionRawMutex, HidReport, HID_Q>;

/// Consumer handle — held by [`hid_loop`].
pub type HidReceiver = Receiver<'static, CriticalSectionRawMutex, HidReport, HID_Q>;

/// Largest HID report we ever write, in bytes: the keyboard report — Report ID
/// (1) + modifiers (1) + reserved (1) + six keycodes (6) = 9. Sizes the
/// [`HidWriter`]'s `const N` buffer; the consumer report is shorter.
pub const REPORT_LEN: usize = 9;

/// Report IDs — the first byte of every report, matching [`REPORT_DESCRIPTOR`].
const REPORT_ID_KEYBOARD: u8 = 1;
const REPORT_ID_CONSUMER: u8 = 3;

/// Gap between a tap's press and release report. Long enough for any host to
/// see a distinct key-down then key-up, short enough to feel instant.
const TAP: Duration = Duration::from_millis(12);

/// USB-HID report descriptor: a keyboard collection (Report ID 1) and a
/// consumer-control collection (Report ID 3). Hand-rolled (USB HID 1.11 item
/// encoding) rather than pulled from a macro crate, to keep the wire format
/// fully owned and reviewable next to the writer that feeds it.
#[rustfmt::skip]
pub const REPORT_DESCRIPTOR: &[u8] = &[
    // ── Keyboard (Report ID 1) ──────────────────────────────────────────
    0x05, 0x01,        // Usage Page (Generic Desktop)
    0x09, 0x06,        // Usage (Keyboard)
    0xA1, 0x01,        // Collection (Application)
    0x85, REPORT_ID_KEYBOARD, //   Report ID (1)
    //   Modifier byte: 8 bits, one per modifier key (0xE0..=0xE7).
    0x05, 0x07,        //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0,        //   Usage Minimum (Left Control)
    0x29, 0xE7,        //   Usage Maximum (Right GUI)
    0x15, 0x00,        //   Logical Minimum (0)
    0x25, 0x01,        //   Logical Maximum (1)
    0x75, 0x01,        //   Report Size (1)
    0x95, 0x08,        //   Report Count (8)
    0x81, 0x02,        //   Input (Data,Var,Abs) — modifier bits
    //   Reserved byte (boot-keyboard layout).
    0x95, 0x01,        //   Report Count (1)
    0x75, 0x08,        //   Report Size (8)
    0x81, 0x01,        //   Input (Const) — reserved
    //   Six key codes (array of usage IDs, 0 = no key).
    0x95, 0x06,        //   Report Count (6)
    0x75, 0x08,        //   Report Size (8)
    0x15, 0x00,        //   Logical Minimum (0)
    0x26, 0xFF, 0x00,  //   Logical Maximum (255)
    0x05, 0x07,        //   Usage Page (Keyboard/Keypad)
    0x19, 0x00,        //   Usage Minimum (0)
    0x29, 0xFF,        //   Usage Maximum (255)
    0x81, 0x00,        //   Input (Data,Array) — key array
    0xC0,              // End Collection
    // ── Consumer control (Report ID 3) ──────────────────────────────────
    0x05, 0x0C,        // Usage Page (Consumer)
    0x09, 0x01,        // Usage (Consumer Control)
    0xA1, 0x01,        // Collection (Application)
    0x85, REPORT_ID_CONSUMER, //   Report ID (3)
    0x15, 0x00,        //   Logical Minimum (0)
    0x26, 0xFF, 0x03,  //   Logical Maximum (0x03FF)
    0x19, 0x00,        //   Usage Minimum (0)
    0x2A, 0xFF, 0x03,  //   Usage Maximum (0x03FF)
    0x75, 0x10,        //   Report Size (16)
    0x95, 0x01,        //   Report Count (1)
    0x81, 0x00,        //   Input (Data,Array) — one 16-bit usage
    0xC0,              // End Collection
];

/// Drive the HID endpoint: receive [`HidReport`]s and write each as a
/// press-then-release tap. Writes are best-effort — if the host isn't reading
/// (endpoint not yet configured, or a transient `EndpointError`), the report is
/// dropped rather than blocking the router's producer.
pub async fn hid_loop<D: Driver<'static>>(
    mut writer: HidWriter<'static, D, REPORT_LEN>,
    rx: HidReceiver,
) -> ! {
    loop {
        match rx.receive().await {
            HidReport::Key { keycode, modifiers } => {
                let press = [REPORT_ID_KEYBOARD, modifiers, 0, keycode, 0, 0, 0, 0, 0];
                let _ = writer.write(&press).await;
                Timer::after(TAP).await;
                let release = [REPORT_ID_KEYBOARD, 0, 0, 0, 0, 0, 0, 0, 0];
                let _ = writer.write(&release).await;
            }
            HidReport::Consumer { usage } => {
                // 16-bit usage, little-endian.
                let press = [REPORT_ID_CONSUMER, usage as u8, (usage >> 8) as u8];
                let _ = writer.write(&press).await;
                Timer::after(TAP).await;
                let release = [REPORT_ID_CONSUMER, 0, 0];
                let _ = writer.write(&release).await;
            }
        }
    }
}
