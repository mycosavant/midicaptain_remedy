"""
MIDICaptain Remedy - MIDI Handler

Handles USB-MIDI and DIN-MIDI (UART) communication.
Includes SysEx support with Roland checksum calculation.
"""

import busio
import usb_midi
import adafruit_midi
from adafruit_midi.control_change import ControlChange
from adafruit_midi.program_change import ProgramChange
from adafruit_midi.note_on import NoteOn
from adafruit_midi.note_off import NoteOff
from adafruit_midi.pitch_bend import PitchBend
from adafruit_midi.system_exclusive import SystemExclusive
from adafruit_midi.timing_clock import TimingClock

from . import pins


# ═══════════════════════════════════════════════════════════════════════════════
# ROLAND SYSEX HELPERS
# ═══════════════════════════════════════════════════════════════════════════════

def roland_checksum(data):
    """
    Calculate Roland-style checksum.

    Algorithm:
    1. Sum all bytes in data
    2. AND with 0x7F (keep lower 7 bits)
    3. Subtract from 128
    4. AND with 0x7F again
    """
    accum = sum(data) & 0x7F
    return (128 - accum) & 0x7F


def build_roland_sysex(model_id, address, data, device_id=0x00, operation=0x12):
    """
    Build a complete Roland SysEx message.

    Args:
        model_id: Model ID bytes (list)
        address: 4-byte address (list)
        data: Data bytes to send (list)
        device_id: Device ID (default 0x00)
        operation: 0x11 for query, 0x12 for set

    Returns:
        List of bytes for the complete SysEx message
    """
    # Ensure address is 4 bytes
    addr = list(address) if len(address) == 4 else [0, 0, 0, 0]

    # Build payload for checksum (address + data)
    payload = addr + list(data)
    checksum = roland_checksum(payload)

    # Build complete message (without F0/F7 - adafruit_midi adds those)
    msg = [0x41, device_id] + list(model_id) + [operation] + payload + [checksum]
    return msg


def encode_roland_11bit(value):
    """
    Encode a value using Roland's 11-bit encoding.

    Used for delay time and other large values (0-2000).
    Returns 2 bytes.
    """
    value = max(0, min(2000, value))
    high = (value >> 7) & 0x0F
    low = value & 0x7F
    return [high, low]


def decode_roland_11bit(high, low):
    """Decode Roland's 11-bit encoding back to integer."""
    return ((high & 0x0F) << 7) | (low & 0x7F)


# ═══════════════════════════════════════════════════════════════════════════════
# MIDI INTERFACE
# ═══════════════════════════════════════════════════════════════════════════════

class MidiInterface:
    """
    MIDI interface handling both USB and DIN (UART) MIDI.
    """

    # Katana model ID
    KATANA_MODEL_ID = [0x00, 0x00, 0x00, 0x33]

    def __init__(self, usb_enabled=True, din_enabled=True, default_channel=1):
        self.default_channel = default_channel
        self._usb_midi = None
        self._din_midi = None
        self._uart = None

        # Initialize USB MIDI
        if usb_enabled:
            try:
                self._usb_midi = adafruit_midi.MIDI(
                    midi_in=usb_midi.ports[0],
                    midi_out=usb_midi.ports[1],
                    in_channel=default_channel - 1,  # 0-indexed
                    out_channel=default_channel - 1
                )
            except Exception as e:
                print(f"USB MIDI init failed: {e}")

        # Initialize DIN MIDI (UART)
        if din_enabled:
            try:
                self._uart = busio.UART(
                    pins.MIDI_TX,
                    pins.MIDI_RX,
                    baudrate=pins.MIDI_BAUDRATE,
                    timeout=0.001
                )
                self._din_midi = adafruit_midi.MIDI(
                    midi_in=self._uart,
                    midi_out=self._uart,
                    in_channel=default_channel - 1,
                    out_channel=default_channel - 1
                )
            except Exception as e:
                print(f"DIN MIDI init failed: {e}")

        # Callback for incoming messages
        self._message_callback = None

        # SysEx response buffer
        self._sysex_response = None

    def set_channel(self, channel):
        """Set default MIDI channel (1-16)."""
        self.default_channel = channel
        ch = channel - 1  # Convert to 0-indexed

        if self._usb_midi:
            self._usb_midi.in_channel = ch
            self._usb_midi.out_channel = ch
        if self._din_midi:
            self._din_midi.in_channel = ch
            self._din_midi.out_channel = ch

    def set_message_callback(self, callback):
        """Set callback for incoming MIDI messages."""
        self._message_callback = callback

    # ─────────────────────────────────────────────────────────────────────────
    # SENDING MESSAGES
    # ─────────────────────────────────────────────────────────────────────────

    def send_cc(self, channel, cc, value):
        """Send Control Change message."""
        msg = ControlChange(cc, value)
        self._send(msg, channel)

    def send_pc(self, channel, program):
        """Send Program Change message."""
        msg = ProgramChange(program)
        self._send(msg, channel)

    def send_note_on(self, channel, note, velocity=127):
        """Send Note On message."""
        msg = NoteOn(note, velocity)
        self._send(msg, channel)

    def send_note_off(self, channel, note, velocity=0):
        """Send Note Off message."""
        msg = NoteOff(note, velocity)
        self._send(msg, channel)

    def send_sysex(self, data):
        """
        Send raw SysEx message.

        Args:
            data: List of bytes (without F0/F7 delimiters)
        """
        msg = SystemExclusive(manufacturer_id=bytes([data[0]]), data=bytes(data[1:]))
        self._send(msg)

    def send_sysex_param(self, address, value, model_id=None):
        """
        Send a SysEx parameter set command (Roland format).

        Args:
            address: 4-byte parameter address
            value: Value to set (int or list of bytes)
            model_id: Optional model ID override
        """
        model = model_id or self.KATANA_MODEL_ID

        # Convert value to list if needed
        if isinstance(value, int):
            data = [value]
        else:
            data = list(value)

        sysex_data = build_roland_sysex(model, address, data)
        msg = SystemExclusive(
            manufacturer_id=bytes([0x41]),
            data=bytes(sysex_data[1:])  # Skip manufacturer ID (already in msg)
        )
        self._send(msg)

    def query_sysex_param(self, address, length=1, model_id=None):
        """
        Query a SysEx parameter (Roland format).

        Args:
            address: 4-byte parameter address
            length: Number of bytes to request
            model_id: Optional model ID override

        Note: Response will come via message callback
        """
        model = model_id or self.KATANA_MODEL_ID

        # Length as 4-byte value
        length_bytes = [0, 0, 0, length]

        sysex_data = build_roland_sysex(model, address, length_bytes, operation=0x11)
        msg = SystemExclusive(
            manufacturer_id=bytes([0x41]),
            data=bytes(sysex_data[1:])
        )
        self._send(msg)

    def _send(self, msg, channel=None):
        """Send message to both USB and DIN MIDI."""
        # Handle channel for non-SysEx messages
        if channel is not None and hasattr(msg, 'channel'):
            msg.channel = channel - 1  # Convert to 0-indexed

        if self._usb_midi:
            try:
                self._usb_midi.send(msg)
            except Exception as e:
                print(f"USB MIDI send error: {e}")

        if self._din_midi:
            try:
                self._din_midi.send(msg)
            except Exception as e:
                print(f"DIN MIDI send error: {e}")

    # ─────────────────────────────────────────────────────────────────────────
    # RECEIVING MESSAGES
    # ─────────────────────────────────────────────────────────────────────────

    def receive(self):
        """
        Check for and process incoming MIDI messages.

        Returns list of received messages.
        """
        messages = []

        # Check USB MIDI
        if self._usb_midi:
            msg = self._usb_midi.receive()
            if msg:
                messages.append(('usb', msg))

        # Check DIN MIDI
        if self._din_midi:
            msg = self._din_midi.receive()
            if msg:
                messages.append(('din', msg))

        # Process messages
        for source, msg in messages:
            self._process_message(source, msg)

        return messages

    def _process_message(self, source, msg):
        """Process an incoming MIDI message."""
        if self._message_callback:
            # Determine message type
            if isinstance(msg, ControlChange):
                self._message_callback('cc', {
                    'source': source,
                    'channel': msg.channel + 1,
                    'cc': msg.control,
                    'value': msg.value
                })
            elif isinstance(msg, ProgramChange):
                self._message_callback('pc', {
                    'source': source,
                    'channel': msg.channel + 1,
                    'program': msg.patch
                })
            elif isinstance(msg, NoteOn):
                self._message_callback('note_on', {
                    'source': source,
                    'channel': msg.channel + 1,
                    'note': msg.note,
                    'velocity': msg.velocity
                })
            elif isinstance(msg, NoteOff):
                self._message_callback('note_off', {
                    'source': source,
                    'channel': msg.channel + 1,
                    'note': msg.note,
                    'velocity': msg.velocity
                })
            elif isinstance(msg, PitchBend):
                self._message_callback('pitch_bend', {
                    'source': source,
                    'channel': msg.channel + 1,
                    'pitch_bend': msg.pitch_bend  # 0-16383, center=8192
                })
            elif isinstance(msg, SystemExclusive):
                self._message_callback('sysex', {
                    'source': source,
                    'manufacturer_id': msg.manufacturer_id,
                    'data': msg.data
                })
            elif isinstance(msg, TimingClock):
                self._message_callback('clock', {'source': source})

    # ─────────────────────────────────────────────────────────────────────────
    # KATANA-SPECIFIC HELPERS
    # ─────────────────────────────────────────────────────────────────────────

    def katana_enter_editor_mode(self):
        """Enter Katana BTS editor mode."""
        self.send_sysex_param([0x7F, 0x00, 0x00, 0x01], [0x01])

    def katana_exit_editor_mode(self):
        """Exit Katana BTS editor mode."""
        self.send_sysex_param([0x7F, 0x00, 0x00, 0x01], [0x00])

    def katana_recall_preset(self, preset):
        """
        Recall a Katana preset.

        Args:
            preset: 0=Panel, 1-4=CH1-CH4
        """
        self.send_sysex_param([0x00, 0x01, 0x00, 0x00], [0x00, preset])

    def katana_set_amp_type(self, amp_type):
        """
        Set Katana amp type.

        Args:
            amp_type: 0=Acoustic, 1=Clean, 2=Crunch, 3=Lead, 4=Brown
        """
        self.send_sysex_param([0x00, 0x00, 0x04, 0x20], [amp_type])

    def katana_set_gain(self, value):
        """Set Katana gain (0-100)."""
        self.send_sysex_param([0x00, 0x00, 0x04, 0x21], [value])

    def katana_set_volume(self, value):
        """Set Katana volume (0-100)."""
        self.send_sysex_param([0x00, 0x00, 0x04, 0x22], [value])

    def katana_toggle_boost(self, on=None):
        """Toggle or set Katana boost."""
        if on is None:
            # Toggle via CC (simpler)
            self.send_cc(self.default_channel, 16, 127)
        else:
            self.send_sysex_param([0x60, 0x00, 0x00, 0x30], [1 if on else 0])

    def katana_toggle_mod(self, on=None):
        """Toggle or set Katana mod."""
        if on is None:
            self.send_cc(self.default_channel, 17, 127)
        else:
            self.send_sysex_param([0x60, 0x00, 0x01, 0x40], [1 if on else 0])

    def katana_toggle_delay(self, on=None):
        """Toggle or set Katana delay."""
        if on is None:
            self.send_cc(self.default_channel, 19, 127)
        else:
            self.send_sysex_param([0x60, 0x00, 0x05, 0x60], [1 if on else 0])

    def katana_toggle_reverb(self, on=None):
        """Toggle or set Katana reverb."""
        if on is None:
            self.send_cc(self.default_channel, 20, 127)
        else:
            self.send_sysex_param([0x60, 0x00, 0x06, 0x10], [1 if on else 0])

    def katana_set_delay_time(self, ms):
        """Set Katana delay time in milliseconds (1-2000)."""
        encoded = encode_roland_11bit(ms)
        self.send_sysex_param([0x60, 0x00, 0x05, 0x62], encoded)

    def deinit(self):
        """Release MIDI resources."""
        if self._uart:
            self._uart.deinit()
