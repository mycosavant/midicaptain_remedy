"""
MIDICaptain Remedy - Chromatic Tuner Module

Displays tuning data received from the connected amp/device.

The MIDICaptain does NOT do pitch detection itself — it has no audio input.
Instead, it works cooperatively with the connected device (e.g., BOSS Katana):

  1. User presses tuner button → firmware sends CC 25 = 127 to amp
  2. Amp enters tuner mode, detects pitch from guitar audio
  3. Amp sends tuning data back via MIDI (Note On + Pitch Bend)
  4. Firmware displays the note name and deviation on screen

Protocol:
- Tuner mode is toggled via MIDI CC (configurable, default CC#25)
- Note detection via MIDI Note On messages from device
- Pitch deviation via MIDI Pitch Bend (8192 = center = 0 cents)
"""

import time

try:
    import displayio
    import terminalio
    from adafruit_display_text import label
except ImportError:
    pass

from .display import DisplayElement


# ═══════════════════════════════════════════════════════════════════════════════
# CONSTANTS
# ═══════════════════════════════════════════════════════════════════════════════

# Note names (using sharps)
NOTE_NAMES_SHARP = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

# Note names (using flats) - more common in some musical contexts
NOTE_NAMES_FLAT = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"]

# Pitch bend center value (MIDI spec)
PITCH_BEND_CENTER = 8192

# Default cents range for pitch bend (full range = ±200 cents typically)
DEFAULT_PITCH_BEND_CENTS = 200


# ═══════════════════════════════════════════════════════════════════════════════
# TUNER STATE
# ═══════════════════════════════════════════════════════════════════════════════

class TunerState:
    """
    Holds the current tuner state.

    Separated from display logic for clean architecture.
    """

    def __init__(self, reference_pitch=440.0, pitch_bend_range=DEFAULT_PITCH_BEND_CENTS):
        # Configuration
        self.reference_pitch = reference_pitch  # A4 frequency
        self.pitch_bend_range = pitch_bend_range  # Cents per full pitch bend range

        # Current state
        self._active = False
        self._note_number = None  # MIDI note number (0-127)
        self._note_name = ""
        self._octave = None
        self._cents = 0.0  # Deviation in cents (-range to +range)
        self._last_update = 0

        # History for smoothing (optional)
        self._cents_history = []
        self._history_size = 3

        # Callbacks
        self._on_change_callback = None

    @property
    def active(self):
        return self._active

    @active.setter
    def active(self, value):
        if self._active != value:
            self._active = value
            self._notify_change()
            if not value:
                self.clear()

    @property
    def note_name(self):
        return self._note_name

    @property
    def octave(self):
        return self._octave

    @property
    def full_note_name(self):
        """Get full note name with octave (e.g., 'A4', 'C#3')."""
        if self._note_name and self._octave is not None:
            return f"{self._note_name}{self._octave}"
        return ""

    @property
    def cents(self):
        return self._cents

    @property
    def is_in_tune(self):
        """Check if currently in tune (within threshold)."""
        return abs(self._cents) <= 5  # Default 5 cent threshold

    def set_change_callback(self, callback):
        """Set callback for state changes: callback(state)."""
        self._on_change_callback = callback

    def _notify_change(self):
        """Notify listener of state change."""
        if self._on_change_callback:
            self._on_change_callback(self)

    def update_from_note_on(self, note_number, use_flats=False):
        """
        Update state from a MIDI Note On message.

        Args:
            note_number: MIDI note number (0-127)
            use_flats: Use flat names instead of sharps
        """
        self._note_number = note_number
        self._octave = (note_number // 12) - 1  # MIDI octave convention
        note_index = note_number % 12

        names = NOTE_NAMES_FLAT if use_flats else NOTE_NAMES_SHARP
        self._note_name = names[note_index]
        self._last_update = time.monotonic()
        self._notify_change()

    def update_from_pitch_bend(self, pitch_bend_value):
        """
        Update cents deviation from MIDI Pitch Bend.

        Args:
            pitch_bend_value: Raw pitch bend value (0-16383, center=8192)
        """
        # Convert pitch bend to cents
        # Full range is 0-16383, center is 8192
        normalized = (pitch_bend_value - PITCH_BEND_CENTER) / PITCH_BEND_CENTER
        cents = normalized * self.pitch_bend_range

        # Optional: smooth with history
        self._cents_history.append(cents)
        if len(self._cents_history) > self._history_size:
            self._cents_history.pop(0)

        # Use average for stability
        self._cents = sum(self._cents_history) / len(self._cents_history)
        self._last_update = time.monotonic()
        self._notify_change()

    def clear(self):
        """Clear the current note (e.g., on Note Off)."""
        self._note_number = None
        self._note_name = ""
        self._octave = None
        self._cents = 0.0
        self._cents_history.clear()
        self._notify_change()


# ═══════════════════════════════════════════════════════════════════════════════
# TUNER DISPLAY
# ═══════════════════════════════════════════════════════════════════════════════

class TunerDisplay(DisplayElement):
    """
    Lightweight text-only tuner display.

    Uses Labels instead of Rects to avoid bitmap allocations.
    Shows:
    - Note name (large, centered)
    - Cents deviation text
    - Text-based tuning bar indicator
    """

    # Thresholds (in cents)
    IN_TUNE_THRESHOLD = 3
    CLOSE_THRESHOLD = 10

    # Colors (24-bit packed)
    COLOR_IN_TUNE = 0x00FF00   # Green
    COLOR_SHARP = 0xFF0000     # Red
    COLOR_FLAT = 0x0000FF      # Blue
    COLOR_CLOSE = 0xFFFF00     # Yellow
    COLOR_NO_NOTE = 0x888888   # Grey

    # Bar width in characters (must be odd for center mark)
    BAR_CHARS = 21

    def __init__(self, x, y, width, height, font_large=None, state=None):
        super().__init__(x, y, width, height)
        self._font = font_large
        self._state = state or TunerState()

        # Display objects
        self._note_label = None
        self._cents_label = None
        self._bar_label = None

        # Cached values to detect changes
        self._last_note = ""
        self._last_cents = 0
        self._last_color = None

    @property
    def state(self):
        return self._state

    def _get_color(self, cents):
        """Determine display color based on cents deviation."""
        abs_cents = abs(cents)
        if abs_cents <= self.IN_TUNE_THRESHOLD:
            return self.COLOR_IN_TUNE
        elif abs_cents <= self.CLOSE_THRESHOLD:
            return self.COLOR_CLOSE
        elif cents > 0:
            return self.COLOR_SHARP
        else:
            return self.COLOR_FLAT

    def _build_bar(self, cents):
        """Build a text tuning bar. Center='|', marker='#'."""
        mid = self.BAR_CHARS // 2
        # Map ±30 cents to ±mid positions
        clamped = max(-30, min(30, cents))
        pos = mid + int(clamped * mid / 30)
        pos = max(0, min(self.BAR_CHARS - 1, pos))

        chars = ['-'] * self.BAR_CHARS
        chars[mid] = '|'
        chars[pos] = '#'
        return ''.join(chars)

    def create_group(self):
        """Create the displayio group with text-only elements."""
        self._group = displayio.Group()

        note_font = self._font or terminalio.FONT
        note_scale = 1 if self._font else 3

        # Note name (large, centered)
        self._note_label = label.Label(
            note_font, text="--",
            color=self.COLOR_NO_NOTE,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, self.y + 25),
            scale=note_scale
        )
        self._group.append(self._note_label)

        # Cents deviation
        self._cents_label = label.Label(
            terminalio.FONT, text="",
            color=self.COLOR_NO_NOTE,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, self.y + 65),
            scale=2
        )
        self._group.append(self._cents_label)

        # Tuning bar
        self._bar_label = label.Label(
            terminalio.FONT, text=self._build_bar(0),
            color=self.COLOR_NO_NOTE,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, self.y + 95),
            scale=1
        )
        self._group.append(self._bar_label)

        self.clear_dirty()
        return self._group

    def update(self):
        """Update the tuner display based on current state."""
        if not self._group:
            return False

        updated = False
        state = self._state

        # Update note name
        note_display = state.full_note_name if state.full_note_name else "--"
        if note_display != self._last_note:
            self._note_label.text = note_display
            self._last_note = note_display
            updated = True

        # Update cents and bar
        cents = state.cents if state.full_note_name else 0
        new_color = self._get_color(cents) if state.full_note_name else self.COLOR_NO_NOTE

        if int(cents) != int(self._last_cents) or new_color != self._last_color:
            # Update cents text
            if state.full_note_name:
                sign = "+" if cents > 0 else ""
                self._cents_label.text = f"{sign}{int(cents)}c"
            else:
                self._cents_label.text = ""

            # Update bar
            self._bar_label.text = self._build_bar(cents)

            # Update colors
            self._note_label.color = new_color
            self._cents_label.color = new_color
            self._bar_label.color = new_color

            self._last_cents = cents
            self._last_color = new_color
            updated = True

        self.clear_dirty()
        return updated


# ═══════════════════════════════════════════════════════════════════════════════
# TUNER CONTROLLER
# ═══════════════════════════════════════════════════════════════════════════════

class TunerController:
    """
    Main tuner controller.

    Coordinates between MIDI input, tuner state, and display.
    """

    def __init__(self, midi_interface=None, display_manager=None, config=None):
        self.midi = midi_interface
        self.display_manager = display_manager

        # Configuration
        config = config or {}
        self.toggle_cc = config.get('toggle_cc', 25)
        self.use_flats = config.get('use_flats', False)
        reference = config.get('reference_pitch', 440.0)
        pitch_range = config.get('pitch_bend_range', 200)

        # State
        self.state = TunerState(
            reference_pitch=reference,
            pitch_bend_range=pitch_range
        )
        self.state.set_change_callback(self._on_state_change)

        # Display element (created when attached to display manager)
        self._display = None

    def init_display(self, font_large=None):
        """Initialize the tuner display element."""
        if not self.display_manager:
            return

        self._display = TunerDisplay(
            x=0, y=60, width=240, height=120,
            font_large=font_large,
            state=self.state
        )

        # Add to a 'tuner' layer
        self.display_manager.add_element('tuner', self._display, layer='tuner')

        # Initially hidden
        layer = self.display_manager.get_layer('tuner')
        if layer:
            layer.hidden = True

    def toggle(self, force=None):
        """
        Toggle tuner mode on/off.

        Args:
            force: If provided, force to this state (True/False)
        """
        if force is not None:
            self.state.active = bool(force)
        else:
            self.state.active = not self.state.active

        # Update display layer visibility
        if self.display_manager:
            layer = self.display_manager.get_layer('tuner')
            if layer:
                layer.hidden = not self.state.active

            # Also need to hide/show the normal layer
            normal_layer = self.display_manager.get_layer('normal')
            if normal_layer:
                normal_layer.hidden = self.state.active

        # Send MIDI CC to device (e.g., activate amp's tuner)
        if self.midi:
            value = 127 if self.state.active else 0
            self.midi.send_cc(self.midi.default_channel, self.toggle_cc, value)

    def process_midi_message(self, msg_type, data):
        """
        Process incoming MIDI messages for tuner.

        Args:
            msg_type: Message type ('cc', 'note_on', 'note_off', 'pitch_bend')
            data: Message data dict

        Returns:
            True if message was handled by tuner
        """
        if msg_type == 'cc':
            cc = data.get('cc')
            value = data.get('value', 0)

            if cc == self.toggle_cc:
                self.toggle(force=(value > 63))
                return True

        elif msg_type == 'note_on' and self.state.active:
            note = data.get('note')
            velocity = data.get('velocity', 0)

            if note is not None and velocity > 0:
                self.state.update_from_note_on(note, use_flats=self.use_flats)
                return True

        elif msg_type == 'note_off' and self.state.active:
            self.state.clear()
            return True

        elif msg_type == 'pitch_bend' and self.state.active:
            pitch_bend = data.get('pitch_bend')
            if pitch_bend is not None:
                self.state.update_from_pitch_bend(pitch_bend)
                return True

        return False

    def _on_state_change(self, state):
        """Handle state changes."""
        if self._display:
            self._display.mark_dirty()

    def update(self):
        """Update the tuner display if needed."""
        if self._display and self.state.active:
            return self._display.update()
        return False
