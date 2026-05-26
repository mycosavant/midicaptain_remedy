"""
MIDICaptain Remedy - Hardware Abstraction Layer

Provides clean interfaces for input hardware:
- Footswitches with debouncing
- Rotary encoder with acceleration
- NeoPixel LEDs
- Expression pedals with calibration

Note: Display is managed separately by DisplayManager (lib/display.py)
      MIDI is managed separately by MidiInterface (lib/midi.py)
"""

import time
import digitalio
import analogio
import neopixel
import rotaryio

from . import pins


# ═══════════════════════════════════════════════════════════════════════════════
# BUTTON HANDLER
# ═══════════════════════════════════════════════════════════════════════════════

class Button:
    """
    A debounced button with press type detection.

    Uses simple GPIO polling with software debounce (no external libraries).
    Detects: tap, long press
    """

    DEBOUNCE_MS = 20        # Debounce time in milliseconds
    LONG_PRESS_MS = 500     # Time for long press detection

    def __init__(self, pin, name):
        self.name = name
        self._io = digitalio.DigitalInOut(pin)
        self._io.direction = digitalio.Direction.INPUT
        self._io.pull = digitalio.Pull.UP

        # State tracking
        self._state = True          # Current debounced state (True = not pressed)
        self._last_raw = True       # Last raw reading
        self._last_change = 0       # Time of last state change (ms)
        self._press_start = 0       # When current press began
        self._long_press_fired = False

        # Edge detection (cleared each update cycle)
        self._fell = False          # Just pressed
        self._rose = False          # Just released

    def update(self):
        """Update button state with software debounce. Call in main loop."""
        self._fell = False
        self._rose = False

        raw = self._io.value
        now = time.monotonic_ns() // 1_000_000

        if raw != self._last_raw:
            self._last_change = now
            self._last_raw = raw

        if (now - self._last_change) >= self.DEBOUNCE_MS:
            if raw != self._state:
                self._state = raw
                if not raw:  # Fell (pressed)
                    self._fell = True
                    self._press_start = now
                    self._long_press_fired = False
                else:  # Rose (released)
                    self._rose = True

    @property
    def pressed(self):
        """True if button was just pressed this cycle."""
        return self._fell

    @property
    def released(self):
        """True if button was just released this cycle."""
        return self._rose

    @property
    def is_held(self):
        """True if button is currently held down."""
        return not self._state

    @property
    def long_press(self):
        """True if button has been held for LONG_PRESS_MS (fires once)."""
        if self.is_held and not self._long_press_fired:
            now = time.monotonic_ns() // 1_000_000
            if now - self._press_start >= self.LONG_PRESS_MS:
                self._long_press_fired = True
                return True
        return False

    @property
    def hold_time_ms(self):
        """How long the button has been held (in ms)."""
        if self.is_held:
            now = time.monotonic_ns() // 1_000_000
            return now - self._press_start
        return 0

    def deinit(self):
        """Release hardware resources."""
        self._io.deinit()


class ButtonManager:
    """
    Manages all footswitches and the encoder button.
    """

    def __init__(self):
        self.buttons = {}

        # Initialize footswitches
        for name, pin in pins.FOOTSWITCHES:
            self.buttons[name] = Button(pin, name)

        # Encoder button
        self.buttons['encoder'] = Button(pins.ENCODER_SW, 'encoder')

    def update(self):
        """Update all buttons. Call in main loop."""
        for button in self.buttons.values():
            button.update()

    def get(self, name):
        """Get a button by name."""
        return self.buttons.get(name)

    def get_events(self):
        """
        Get all button events that occurred this cycle.

        Returns list of (button_name, event_type) tuples.
        Event types: 'press', 'release', 'long_press'
        """
        events = []
        for name, button in self.buttons.items():
            if button.pressed:
                events.append((name, 'press'))
            if button.released:
                events.append((name, 'release'))
            if button.long_press:
                events.append((name, 'long_press'))
        return events

    def deinit(self):
        """Release all button resources."""
        for button in self.buttons.values():
            button.deinit()


# ═══════════════════════════════════════════════════════════════════════════════
# ROTARY ENCODER
# ═══════════════════════════════════════════════════════════════════════════════

class Encoder:
    """
    Rotary encoder with position tracking and acceleration.
    """

    def __init__(self, sensitivity=1, acceleration=True):
        self._encoder = rotaryio.IncrementalEncoder(pins.ENCODER_A, pins.ENCODER_B)
        self._last_position = 0
        self._last_time = time.monotonic_ns()
        self.sensitivity = sensitivity
        self.acceleration = acceleration

    @property
    def position(self):
        """Current encoder position."""
        return self._encoder.position

    def get_delta(self):
        """
        Get change in position since last call.

        Returns delta (can be negative for counter-clockwise).
        Applies acceleration if enabled.
        """
        current = self._encoder.position
        delta = current - self._last_position

        if delta == 0:
            return 0

        # Apply acceleration based on rotation speed
        if self.acceleration:
            now = time.monotonic_ns()
            time_delta = (now - self._last_time) / 1_000_000  # ms
            self._last_time = now

            if time_delta < 50:  # Very fast rotation
                delta *= 4
            elif time_delta < 100:
                delta *= 2

        self._last_position = current
        return delta * self.sensitivity

    def deinit(self):
        """Release encoder resources."""
        self._encoder.deinit()


# ═══════════════════════════════════════════════════════════════════════════════
# NEOPIXEL LEDs
# ═══════════════════════════════════════════════════════════════════════════════

class LEDs:
    """
    NeoPixel LED controller with per-button addressing.
    """

    def __init__(self, brightness=0.5):
        self._pixels = neopixel.NeoPixel(
            pins.NEOPIXEL_PIN,
            pins.NEOPIXEL_COUNT,
            brightness=brightness,
            auto_write=False
        )
        self._brightness = brightness

    def set_brightness(self, brightness):
        """Set global LED brightness (0.0 - 1.0)."""
        self._brightness = max(0.0, min(1.0, brightness))
        self._pixels.brightness = self._brightness

    def set_button_color(self, button_name, color, show=True):
        """
        Set color for all LEDs on a button.

        Args:
            button_name: Button identifier ('A', 'B', '1', etc.)
            color: RGB tuple (r, g, b) with values 0-255
            show: If True, update display immediately
        """
        if button_name not in pins.LED_MAP:
            return

        indices = pins.LED_MAP[button_name]
        for idx in indices:
            self._pixels[idx] = color

        if show:
            self._pixels.show()

    def set_button_colors(self, colors_dict, show=True):
        """
        Set colors for multiple buttons at once.

        Args:
            colors_dict: Dict mapping button names to RGB colors
            show: If True, update display after all changes
        """
        for button_name, color in colors_dict.items():
            self.set_button_color(button_name, color, show=False)

        if show:
            self._pixels.show()

    def set_all(self, color, show=True):
        """Set all LEDs to same color."""
        self._pixels.fill(color)
        if show:
            self._pixels.show()

    def clear(self, show=True):
        """Turn off all LEDs."""
        self.set_all((0, 0, 0), show)

    def show(self):
        """Update LED display."""
        self._pixels.show()

    def __setitem__(self, idx, color):
        """Set individual LED by index."""
        self._pixels[idx] = color

    def __getitem__(self, idx):
        """Get individual LED color by index."""
        return self._pixels[idx]

    def deinit(self):
        """Release LED resources."""
        self.clear()
        self._pixels.deinit()


# ═══════════════════════════════════════════════════════════════════════════════
# EXPRESSION PEDALS
# ═══════════════════════════════════════════════════════════════════════════════

class ExpressionPedal:
    """
    Analog expression pedal with calibration and response curves.
    """

    def __init__(self, pin, pedal_id=1):
        self._adc = analogio.AnalogIn(pin)
        self.pedal_id = pedal_id

        # Calibration values (raw ADC)
        self.cal_min = 0
        self.cal_max = 65535
        self.deadzone = 2  # Percent deadzone at ends

        # Response curve: 'linear', 'log', 'exp'
        self.curve = 'linear'

        # Output range
        self.out_min = 0
        self.out_max = 127

        # Smoothing
        self._last_value = None
        self._threshold = 1  # Minimum change to report

    @property
    def raw_value(self):
        """Get raw ADC value (0-65535)."""
        return self._adc.value

    @property
    def value(self):
        """
        Get processed value (out_min to out_max) with calibration and curve.
        """
        raw = self.raw_value

        # Apply calibration
        cal_range = self.cal_max - self.cal_min
        normalized = (raw - self.cal_min) / cal_range if cal_range > 0 else 0
        normalized = max(0.0, min(1.0, normalized))

        # Apply deadzone
        dz = self.deadzone / 100.0
        if normalized < dz:
            normalized = 0.0
        elif normalized > (1.0 - dz):
            normalized = 1.0
        else:
            normalized = (normalized - dz) / (1.0 - 2 * dz)

        # Apply response curve
        if self.curve == 'log':
            # Logarithmic (audio taper)
            normalized = normalized ** 0.5
        elif self.curve == 'exp':
            # Exponential
            normalized = normalized ** 2
        # 'linear' uses normalized directly

        # Scale to output range
        out_range = self.out_max - self.out_min
        return int(self.out_min + normalized * out_range)

    def get_if_changed(self):
        """
        Get value only if it changed significantly.

        Returns value or None if no significant change.
        """
        current = self.value

        if self._last_value is None:
            self._last_value = current
            return current

        if abs(current - self._last_value) >= self._threshold:
            self._last_value = current
            return current

        return None

    def calibrate_min(self):
        """Set current position as minimum."""
        self.cal_min = self.raw_value

    def calibrate_max(self):
        """Set current position as maximum."""
        self.cal_max = self.raw_value

    def deinit(self):
        """Release ADC resources."""
        self._adc.deinit()


class ExpressionManager:
    """Manages both expression pedals."""

    def __init__(self):
        self.pedal1 = ExpressionPedal(pins.EXPRESSION_1, 1)
        self.pedal2 = ExpressionPedal(pins.EXPRESSION_2, 2)

    def get_values(self):
        """Get current values if changed. Returns dict."""
        result = {}

        val1 = self.pedal1.get_if_changed()
        if val1 is not None:
            result[1] = val1

        val2 = self.pedal2.get_if_changed()
        if val2 is not None:
            result[2] = val2

        return result

    def deinit(self):
        self.pedal1.deinit()
        self.pedal2.deinit()


# ═══════════════════════════════════════════════════════════════════════════════
# HARDWARE MANAGER
# ═══════════════════════════════════════════════════════════════════════════════

class Hardware:
    """
    Central hardware manager providing access to all peripherals.
    """

    def __init__(self, config=None):
        self.config = config

        # Get brightness settings from config
        led_brightness = 0.5

        if config:
            led_brightness = config.led_brightness / 100.0

        # Initialize subsystems
        self.buttons = ButtonManager()
        self.encoder = Encoder()
        self.leds = LEDs(brightness=led_brightness)
        self.expression = ExpressionManager()
        # Note: Display is managed by DisplayManager in display.py
        # (initializing SPI here would conflict with displayio)

    def update(self):
        """
        Update all input hardware. Call in main loop.

        Returns dict of events that occurred.
        """
        self.buttons.update()

        events = {
            'buttons': self.buttons.get_events(),
            'encoder': self.encoder.get_delta(),
            'expression': self.expression.get_values(),
        }

        return events

    def deinit(self):
        """Release all hardware resources."""
        self.buttons.deinit()
        self.encoder.deinit()
        self.leds.deinit()
        self.expression.deinit()
