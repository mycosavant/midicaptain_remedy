"""
MIDICaptain Hardware Test - Safe minimal test
Copy this to the device as code.py
"""
import board
import digitalio
import neopixel
import time

print("\n" + "="*40)
print("  MIDICaptain Remedy - Hardware Test")
print("="*40 + "\n")

# ── NeoPixels ──────────────────────────────────────────
print("Initializing LEDs...")
pixels = neopixel.NeoPixel(board.GP7, 30, brightness=0.3, auto_write=False)

# Quick test - all green
pixels.fill((0, 50, 0))
pixels.show()
print("  LEDs: OK (should be dim green)")

time.sleep(1)

# ── Buttons ────────────────────────────────────────────
print("Initializing buttons...")
BUTTONS = {
    '1': board.GP1, '2': board.GP25, '3': board.GP24, '4': board.GP23,
    'A': board.GP9, 'B': board.GP10, 'C': board.GP11, 'D': board.GP18,
    'up': board.GP20, 'down': board.GP19, 'enc': board.GP0,
}

buttons = {}
for name, pin in BUTTONS.items():
    b = digitalio.DigitalInOut(pin)
    b.direction = digitalio.Direction.INPUT
    b.pull = digitalio.Pull.UP
    buttons[name] = b

print(f"  Buttons: OK ({len(buttons)} initialized)")

# ── LED Colors per button ──────────────────────────────
LED_START = {'1': 0, '2': 3, '3': 6, '4': 9, 'A': 12, 'B': 15, 'C': 18, 'D': 21, 'up': 24, 'down': 27}
COLORS = {
    '1': (255,0,0), '2': (255,128,0), '3': (255,255,0), '4': (0,255,0),
    'A': (0,255,255), 'B': (0,0,255), 'C': (128,0,255), 'D': (255,0,255),
    'up': (255,255,255), 'down': (100,100,100), 'enc': (255,200,0),
}

# ── Startup animation ──────────────────────────────────
print("Running LED animation...")
for name in ['1', '2', '3', '4', 'A', 'B', 'C', 'D', 'up', 'down']:
    start = LED_START[name]
    color = COLORS[name]
    for i in range(3):
        pixels[start + i] = color
    pixels.show()
    time.sleep(0.1)

time.sleep(0.5)

# Dim all
for i in range(30):
    pixels[i] = (10, 10, 10)
pixels.show()

# ── Main loop ──────────────────────────────────────────
print("\n" + "="*40)
print("  READY - Press buttons to test!")
print("  (Watch LEDs and serial output)")
print("="*40 + "\n")

states = {name: True for name in buttons}

while True:
    for name, btn in buttons.items():
        current = btn.value
        if current != states[name]:
            states[name] = current

            if name in LED_START:
                start = LED_START[name]

                if not current:  # Pressed
                    print(f">>> PRESSED: {name}")
                    for i in range(3):
                        pixels[start + i] = COLORS[name]
                else:  # Released
                    print(f"    released: {name}")
                    for i in range(3):
                        pixels[start + i] = (10, 10, 10)

                pixels.show()
            else:
                # Encoder button
                if not current:
                    print(f">>> PRESSED: {name}")
                else:
                    print(f"    released: {name}")

    time.sleep(0.01)
