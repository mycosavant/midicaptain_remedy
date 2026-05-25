"""
MIDICaptain Remedy - Hardware Pin Definitions

Pin assignments for the Paint Audio MIDI Captain (RP2040-based)
Based on reverse engineering documentation.

Hardware:
- Microcontroller: Raspberry Pi Pico (RP2040)
- Display: ST7789 240x240 TFT LCD
- LEDs: 30 NeoPixels (3 per footswitch, 10 switches)
- Switches: 10 footswitches + 1 encoder button
- Encoder: Rotary with push button
- Expression: 2 analog inputs
- MIDI: UART TX/RX (5-pin DIN) + USB-MIDI
"""

import board

# ═══════════════════════════════════════════════════════════════════════════════
# FOOTSWITCHES
# ═══════════════════════════════════════════════════════════════════════════════
# Active LOW (pull-up, pressed = False)

# Numbered row (bottom)
SWITCH_1 = board.GP1      # Also used for boot mode detection
SWITCH_2 = board.GP25
SWITCH_3 = board.GP24
SWITCH_4 = board.GP23

# Lettered row (top)
SWITCH_A = board.GP9
SWITCH_B = board.GP10
SWITCH_C = board.GP11
SWITCH_D = board.GP18

# Navigation
SWITCH_UP = board.GP20
SWITCH_DOWN = board.GP19

# All footswitches in order (for iteration)
FOOTSWITCHES = [
    ('1', SWITCH_1),
    ('2', SWITCH_2),
    ('3', SWITCH_3),
    ('4', SWITCH_4),
    ('A', SWITCH_A),
    ('B', SWITCH_B),
    ('C', SWITCH_C),
    ('D', SWITCH_D),
    ('up', SWITCH_UP),
    ('down', SWITCH_DOWN),
]

# ═══════════════════════════════════════════════════════════════════════════════
# ROTARY ENCODER
# ═══════════════════════════════════════════════════════════════════════════════

ENCODER_A = board.GP2       # Phase A
ENCODER_B = board.GP3       # Phase B
ENCODER_SW = board.GP0      # Push button (active LOW)

# ═══════════════════════════════════════════════════════════════════════════════
# NEOPIXEL LEDs
# ═══════════════════════════════════════════════════════════════════════════════

NEOPIXEL_PIN = board.GP7
NEOPIXEL_COUNT = 30         # 3 LEDs per footswitch × 10 switches

# LED indices for each footswitch (3 LEDs per switch)
# NeoPixel chain order: 1,2,3,4 (top row) then up,A,B,C,D,down (bottom row)
LED_MAP = {
    '1': (0, 1, 2),
    '2': (3, 4, 5),
    '3': (6, 7, 8),
    '4': (9, 10, 11),
    'up': (12, 13, 14),
    'A': (15, 16, 17),
    'B': (18, 19, 20),
    'C': (21, 22, 23),
    'D': (24, 25, 26),
    'down': (27, 28, 29),
}

# ═══════════════════════════════════════════════════════════════════════════════
# DISPLAY (ST7789 240x240)
# ═══════════════════════════════════════════════════════════════════════════════

# SPI bus
SPI_CLK = board.GP14
SPI_MOSI = board.GP15
# SPI_MISO not used (display is write-only)

# Display control
TFT_CS = board.GP13         # Chip select
TFT_DC = board.GP12         # Data/Command
TFT_PWM = board.GP8         # Backlight PWM

# Display parameters
DISPLAY_WIDTH = 240
DISPLAY_HEIGHT = 240
DISPLAY_ROTATION = 180      # Mounted upside down
SPI_BAUDRATE = 24_000_000   # 24 MHz

# ═══════════════════════════════════════════════════════════════════════════════
# MIDI UART
# ═══════════════════════════════════════════════════════════════════════════════

MIDI_TX = board.GP16
MIDI_RX = board.GP17
MIDI_BAUDRATE = 31250       # Standard MIDI baud rate

# ═══════════════════════════════════════════════════════════════════════════════
# ANALOG INPUTS
# ═══════════════════════════════════════════════════════════════════════════════

EXPRESSION_1 = board.GP27   # A1 - Expression pedal 1
EXPRESSION_2 = board.GP28   # A2 - Expression pedal 2

# Battery voltage (GP29/A3) - not exposed on all CircuitPython builds
try:
    BATTERY_VOLTAGE = board.GP29
except AttributeError:
    BATTERY_VOLTAGE = None

# ADC reference voltage
ADC_REF_VOLTAGE = 3.3

# ═══════════════════════════════════════════════════════════════════════════════
# UNUSED/RESERVED GPIO
# ═══════════════════════════════════════════════════════════════════════════════
# These pins are available for future expansion:
# GP4, GP5, GP6, GP21, GP22, GP26
