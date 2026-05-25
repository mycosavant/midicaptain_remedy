"""
bootsel_hammer.py - Catch a flaky RP2040 USB-CDC port and force the UF2 bootloader.

Run on WINDOWS (not WSL - WSL can't see COM ports):
    py -m pip install pyserial      # once, if needed
    py bootsel_hammer.py

It watches for an RP2040 serial port (Raspberry Pi VID 0x2E8A, or a
TinyUSB/Pico/MIDICaptain CDC port) and performs the 1200-baud "touch"
the moment it appears - exactly what MIDICAPTAINBOOT.HTML does, but in a
tight loop so it cannot miss a sub-second enumeration window.

When the touch succeeds the device re-enumerates as the RPI-RP2 mass-storage
drive and the COM port disappears - that's your success signal. Then drag the
firmware .uf2 onto RPI-RP2.

Tip: disconnect other USB-MIDI gear first so we only target the pedal.
Optional: pass a specific port to only touch that one:  py bootsel_hammer.py COM39
"""

import sys
import time

try:
    import serial
    from serial.tools import list_ports
except ImportError:
    sys.exit("pyserial not installed. Run:  py -m pip install pyserial")

RP2040_VID = 0x2E8A  # Raspberry Pi (Pico SDK / TinyUSB default vendor id)
KEYWORDS = ("tinyusb", "pico", "rp2", "midicaptain", "usb serial")
POLL_S = 0.12        # how often to rescan ports
TOUCH_HOLD_S = 0.25  # how long to hold the 1200-baud open before closing


def is_target(p):
    if p.vid == RP2040_VID:
        return True
    desc = (p.description or "").lower()
    return any(k in desc for k in KEYWORDS)


def touch(port_name):
    """Open at 1200 baud, drop DTR, close -> triggers RP2040 reset to UF2 bootloader."""
    try:
        s = serial.Serial(port_name, baudrate=1200)
        try:
            s.dtr = False
        except Exception:
            pass
        time.sleep(TOUCH_HOLD_S)
        s.close()
        return True
    except serial.SerialException:
        # Port may have vanished mid-touch (device reset) - that's often success.
        return False


def main():
    forced = sys.argv[1] if len(sys.argv) > 1 else None
    if forced:
        print(f"Targeting only {forced}. Power-cycle the pedal now. Ctrl+C to stop.\n")
    else:
        print("Watching for an RP2040 CDC port and hammering the 1200-baud reset.")
        print("Power-cycle / replug the pedal repeatedly. Ctrl+C to stop.\n")

    last_ports = set()
    while True:
        ports = list_ports.comports()
        names = {p.device for p in ports}

        # Report appear/disappear so you can see the window open and close.
        for gone in last_ports - names:
            print(f"[{time.strftime('%H:%M:%S')}] port GONE: {gone}  "
                  f"(if RPI-RP2 drive just appeared, you're in the bootloader!)")
        for new in names - last_ports:
            print(f"[{time.strftime('%H:%M:%S')}] port NEW : {new}")
        last_ports = names

        if forced:
            if forced in names:
                print(f"[{time.strftime('%H:%M:%S')}] touching {forced} @1200")
                touch(forced)
        else:
            candidates = [p for p in ports if is_target(p)]
            for p in candidates:
                print(f"[{time.strftime('%H:%M:%S')}] touching {p.device} @1200 "
                      f"({p.description}, vid={p.vid:#06x} pid={p.pid:#06x})")
                touch(p.device)

        time.sleep(POLL_S)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nStopped.")
