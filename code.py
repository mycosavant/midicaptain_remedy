"""
MIDICaptain Remedy - Wokwi Test Script

Simple hardware test for Wokwi simulator.
Tests buttons, LEDs, and expression pedals without MIDI/display dependencies.
"""

import board
import digitalio
import neopixel
import time
from analogio import AnalogIn

# ═══════════════════════════════════════════════════════════════════════════════
# HARDWARE SETUP
# ═══════════════════════════════════════════════════════════════════════════════

print("\n=== MIDICaptain Remedy - Hardware Test ===\n")

# NeoPixels (10 LEDs for Wokwi test - one per switch)
NUM_PIXELS = 10
pixels = neopixel.NeoPixel(board.GP7, NUM_PIXELS, brightness=0.3, auto_write=False)

# LED mapping: which pixel belongs to which button (simplified for Wokwi)
LED_MAP = {
    '1': (0,),
    '2': (1,),
    '3': (2,),
    '4': (3,),
    'A': (4,),
    'B': (5,),
    'C': (6,),
    'D': (7,),
    'up': (8,),
    'down': (9,),
}

# Button colors
COLORS = {
    '1': (255, 0, 0),      # Red
    '2': (255, 128, 0),    # Orange
    '3': (255, 255, 0),    # Yellow
    '4': (0, 255, 0),      # Green
    'A': (0, 255, 255),    # Cyan
    'B': (0, 0, 255),      # Blue
    'C': (128, 0, 255),    # Purple
    'D': (255, 0, 255),    # Magenta
    'up': (255, 255, 255), # White
    'down': (128, 128, 128), # Grey
    'enc': (255, 192, 0),  # Amber
}

# Button GPIO mapping
BUTTON_PINS = {
    '1': board.GP1,
    '2': board.GP25,
    '3': board.GP24,
    '4': board.GP23,
    'A': board.GP9,
    'B': board.GP10,
    'C': board.GP11,
    'D': board.GP18,
    'up': board.GP20,
    'down': board.GP19,
    'enc': board.GP0,
}

# Initialize buttons
buttons = {}
for name, pin in BUTTON_PINS.items():
    btn = digitalio.DigitalInOut(pin)
    btn.direction = digitalio.Direction.INPUT
    btn.pull = digitalio.Pull.UP
    buttons[name] = btn

print("Buttons initialized: 1 2 3 4 A B C D up down enc")

# Initialize expression pedals
exp1 = AnalogIn(board.GP27)
exp2 = AnalogIn(board.GP28)
print("Expression pedals initialized: EXP1 EXP2")

# ═══════════════════════════════════════════════════════════════════════════════
# HELPER FUNCTIONS
# ═══════════════════════════════════════════════════════════════════════════════

def set_button_led(name, color, brightness=1.0):
    """Set all 3 LEDs for a button to a color."""
    if name in LED_MAP:
        r = int(color[0] * brightness)
        g = int(color[1] * brightness)
        b = int(color[2] * brightness)
        for idx in LED_MAP[name]:
            pixels[idx] = (r, g, b)

def map_value(value, in_min, in_max, out_min, out_max):
    """Map a value from one range to another."""
    return (value - in_min) * (out_max - out_min) // (in_max - in_min) + out_min

# ═══════════════════════════════════════════════════════════════════════════════
# STARTUP ANIMATION
# ═══════════════════════════════════════════════════════════════════════════════

print("\nStartup animation...")

# Sweep animation
for name in ['1', '2', '3', '4', 'A', 'B', 'C', 'D', 'up', 'down']:
    set_button_led(name, COLORS[name])
    pixels.show()
    time.sleep(0.08)

time.sleep(0.3)

# Dim all to idle state
for name in LED_MAP.keys():
    set_button_led(name, COLORS.get(name, (128, 128, 128)), brightness=0.15)
pixels.show()

print("\n=== Ready! Press buttons to test ===\n")

# ═══════════════════════════════════════════════════════════════════════════════
# MAIN LOOP
# ═══════════════════════════════════════════════════════════════════════════════

# Track button states
button_states = {name: True for name in buttons}  # True = not pressed (pull-up)
last_exp1 = 0
last_exp2 = 0

while True:
    # Check buttons
    for name, btn in buttons.items():
        current = btn.value  # False when pressed (active low)

        if current != button_states[name]:
            button_states[name] = current

            if not current:  # Button pressed
                print(f"[PRESS] Button {name}")
                set_button_led(name, COLORS.get(name, (255, 255, 255)), brightness=1.0)
            else:  # Button released
                print(f"[release] Button {name}")
                set_button_led(name, COLORS.get(name, (128, 128, 128)), brightness=0.15)

            pixels.show()

    # Check expression pedals (only print on significant change)
    exp1_val = map_value(exp1.value, 0, 65535, 0, 127)
    exp2_val = map_value(exp2.value, 0, 65535, 0, 127)

    if abs(exp1_val - last_exp1) > 2:
        print(f"[EXP1] {exp1_val}")
        last_exp1 = exp1_val

    if abs(exp2_val - last_exp2) > 2:
        print(f"[EXP2] {exp2_val}")
        last_exp2 = exp2_val

    time.sleep(0.01)
