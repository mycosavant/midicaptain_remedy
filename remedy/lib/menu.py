"""
MIDICaptain Remedy - On-Device Menu System

Encoder-driven settings menu with:
- MIDI channel selection
- LED/display brightness
- Expression pedal calibration
- System info

Triggered by encoder button long-press.
"""

try:
    import terminalio
    from adafruit_display_text import label as text_label
except ImportError:
    pass


# ═══════════════════════════════════════════════════════════════════════════════
# MENU SYSTEM
# ═══════════════════════════════════════════════════════════════════════════════

class MenuItem:
    """A single menu item with label, value, and optional edit behavior."""

    def __init__(self, label, value_fn=None, edit_fn=None, action_fn=None):
        self.label = label
        self._value_fn = value_fn    # Returns current display string
        self._edit_fn = edit_fn      # Called with delta to change value
        self._action_fn = action_fn  # Called on select (for sub-menus/actions)

    @property
    def value(self):
        return self._value_fn() if self._value_fn else ""

    def edit(self, delta):
        if self._edit_fn:
            self._edit_fn(delta)

    def action(self):
        if self._action_fn:
            self._action_fn()
            return True
        return False


class MenuSystem:
    """
    On-device settings menu.

    States:
    - inactive: Menu not shown
    - browsing: Scrolling through items
    - editing: Changing a selected item's value
    - calibrating: Expression pedal calibration wizard
    """

    def __init__(self, display_manager, config, hardware):
        self.display = display_manager
        self.config = config
        self.hardware = hardware

        self._active = False
        self._editing = False
        self._selected = 0
        self._items = []
        self._layer = None
        self._labels = []
        self._cursor = None
        self._status_label = None

        # Calibration state
        self._cal_step = 0  # 0=select pedal, 1=min, 2=max, 3=done
        self._cal_pedal = None

        self._build_menu_items()

    def _build_menu_items(self):
        """Create the menu item list."""
        self._items = [
            MenuItem(
                "MIDI Channel",
                value_fn=lambda: str(self.config.midi_channel),
                edit_fn=self._edit_midi_channel,
            ),
            MenuItem(
                "Display Bright",
                value_fn=lambda: f"{self.config.display_brightness}%",
                edit_fn=self._edit_display_brightness,
            ),
            MenuItem(
                "LED Bright",
                value_fn=lambda: f"{self.config.led_brightness}%",
                edit_fn=self._edit_led_brightness,
            ),
            MenuItem(
                "Cal Pedal 1",
                action_fn=lambda: self._start_calibration(1),
            ),
            MenuItem(
                "Cal Pedal 2",
                action_fn=lambda: self._start_calibration(2),
            ),
            MenuItem(
                "Exit",
                action_fn=self.deactivate,
            ),
        ]

    # ─────────────────────────────────────────────────────────────────────
    # EDIT HANDLERS
    # ─────────────────────────────────────────────────────────────────────

    def _edit_midi_channel(self, delta):
        current = self.config.midi_channel
        new_val = max(1, min(16, current + delta))
        self.config._global.setdefault('midi', {})['channel'] = new_val

    def _edit_display_brightness(self, delta):
        current = self.config.display_brightness
        new_val = max(10, min(100, current + delta * 5))
        self.config._global.setdefault('display', {})['brightness'] = new_val
        if self.display:
            self.display.set_brightness(new_val / 100.0)

    def _edit_led_brightness(self, delta):
        current = self.config.led_brightness
        new_val = max(0, min(100, current + delta * 5))
        self.config._global.setdefault('leds', {})['brightness'] = new_val
        if self.hardware:
            self.hardware.leds.set_brightness(new_val / 100.0)

    # ─────────────────────────────────────────────────────────────────────
    # CALIBRATION
    # ─────────────────────────────────────────────────────────────────────

    def _start_calibration(self, pedal_id):
        self._cal_pedal = pedal_id
        self._cal_step = 1
        self._set_status("Move to MIN\nPress any btn")

    def handle_calibration_button(self):
        """Advance calibration wizard on button press. Returns True if handled."""
        if self._cal_step == 0:
            return False

        pedal = self.hardware.expression.pedal1 if self._cal_pedal == 1 else self.hardware.expression.pedal2

        if self._cal_step == 1:
            pedal.calibrate_min()
            self._cal_step = 2
            self._set_status(f"Min: {pedal.cal_min}\nMove to MAX\nPress any btn")
        elif self._cal_step == 2:
            pedal.calibrate_max()
            self._cal_step = 3
            self._set_status(f"Max: {pedal.cal_max}\nCalibration done!")
            # Save to NVM
            self._save_calibration(self._cal_pedal, pedal.cal_min, pedal.cal_max)
        elif self._cal_step == 3:
            self._cal_step = 0
            self._set_status("")
            self._refresh_display()

        return True

    def _save_calibration(self, pedal_id, cal_min, cal_max):
        """Save calibration to NVM. Layout: [nvm[1-2]=pedal1_min, nvm[3-4]=pedal1_max, etc]."""
        try:
            import microcontroller
            offset = 1 + (pedal_id - 1) * 4  # NVM[0] is reset guard
            # Store as 2 bytes each (big-endian, values 0-65535)
            microcontroller.nvm[offset] = (cal_min >> 8) & 0xFF
            microcontroller.nvm[offset + 1] = cal_min & 0xFF
            microcontroller.nvm[offset + 2] = (cal_max >> 8) & 0xFF
            microcontroller.nvm[offset + 3] = cal_max & 0xFF
        except Exception:
            pass

    @staticmethod
    def load_calibration_from_nvm(hardware):
        """Load saved calibration values on boot. Call from main.py."""
        try:
            import microcontroller
            for pedal_id in (1, 2):
                offset = 1 + (pedal_id - 1) * 4
                cal_min = (microcontroller.nvm[offset] << 8) | microcontroller.nvm[offset + 1]
                cal_max = (microcontroller.nvm[offset + 2] << 8) | microcontroller.nvm[offset + 3]
                # Only apply if values look valid (not all 0xFF from blank NVM)
                if cal_min != 0xFFFF and cal_max != 0xFFFF and cal_max > cal_min:
                    pedal = hardware.expression.pedal1 if pedal_id == 1 else hardware.expression.pedal2
                    pedal.cal_min = cal_min
                    pedal.cal_max = cal_max
        except Exception:
            pass

    # ─────────────────────────────────────────────────────────────────────
    # DISPLAY
    # ─────────────────────────────────────────────────────────────────────

    def _init_display(self):
        """Create the menu display layer."""
        if not self.display or not self.display._display:
            return

        font = terminalio.FONT
        self._layer = self.display.create_layer('menu')

        # Title
        self._layer.append(text_label.Label(
            font, text="SETTINGS",
            color=0xFFFFFF,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, 14),
            scale=2
        ))

        # Divider (text line instead of Rect to save memory)
        self._layer.append(text_label.Label(
            font, text="=" * 38,
            color=0x404040,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, 28),
            scale=1
        ))

        # Menu items
        self._labels = []
        for i, item in enumerate(self._items):
            y = 42 + i * 28
            # Item label
            lbl = text_label.Label(
                font, text=f"  {item.label}",
                color=0xCCCCCC,
                anchor_point=(0, 0.5),
                anchored_position=(10, y),
                scale=1
            )
            self._layer.append(lbl)

            # Value label (right-aligned)
            val = text_label.Label(
                font, text=item.value,
                color=0x00CCCC,
                anchor_point=(1.0, 0.5),
                anchored_position=(230, y),
                scale=1
            )
            self._layer.append(val)

            self._labels.append((lbl, val))

        # Cursor indicator
        self._cursor = text_label.Label(
            font, text=">",
            color=0x00FF00,
            anchor_point=(0, 0.5),
            anchored_position=(4, 42),
            scale=1
        )
        self._layer.append(self._cursor)

        # Status line (for calibration)
        self._status_label = text_label.Label(
            font, text="",
            color=0xFFFF00,
            anchor_point=(0.5, 0.5),
            anchored_position=(120, 220),
            scale=1
        )
        self._layer.append(self._status_label)

        # Initially hidden
        self._layer.hidden = True

    def _refresh_display(self):
        """Update displayed values."""
        if not self._labels:
            return

        for i, item in enumerate(self._items):
            lbl, val = self._labels[i]
            val.text = item.value

            # Highlight selected item
            if i == self._selected:
                lbl.color = 0xFFFFFF
                if self._editing:
                    val.color = 0x00FF00  # Green when editing
                else:
                    val.color = 0x00CCCC
            else:
                lbl.color = 0xCCCCCC
                val.color = 0x888888

        # Move cursor
        if self._cursor:
            y = 42 + self._selected * 28
            self._cursor.anchored_position = (4, y)
            self._cursor.text = ">" if not self._editing else "*"

    def _set_status(self, text):
        if self._status_label:
            self._status_label.text = text

    # ─────────────────────────────────────────────────────────────────────
    # PUBLIC API
    # ─────────────────────────────────────────────────────────────────────

    @property
    def active(self):
        return self._active

    def activate(self):
        """Show the menu."""
        if not self._layer:
            try:
                self._init_display()
            except MemoryError:
                print("  Menu: not enough RAM")
                return
        if not self._layer:
            return

        self._active = True
        self._editing = False
        self._selected = 0
        self._cal_step = 0
        self._layer.hidden = False

        # Hide other layers
        home = self.display.get_layer('home')
        if home:
            home.hidden = True

        self._refresh_display()

    def deactivate(self):
        """Hide the menu."""
        self._active = False
        self._editing = False
        self._cal_step = 0

        if self._layer:
            self._layer.hidden = True

        # Show home layer
        home = self.display.get_layer('home')
        if home:
            home.hidden = False

    def handle_encoder(self, delta):
        """Handle encoder rotation in menu. Returns True if consumed."""
        if not self._active:
            return False

        if self._editing:
            # Edit the selected item's value
            self._items[self._selected].edit(delta)
        else:
            # Navigate menu
            self._selected = max(0, min(len(self._items) - 1, self._selected + delta))

        self._refresh_display()
        return True

    def handle_button(self):
        """Handle encoder button press in menu. Returns True if consumed."""
        if not self._active:
            return False

        # Check calibration first
        if self._cal_step > 0:
            return self.handle_calibration_button()

        item = self._items[self._selected]

        # If item has an action (like calibration or exit), run it
        if item.action():
            return True

        # Toggle editing for items with edit handlers
        if item._edit_fn:
            self._editing = not self._editing
            self._refresh_display()
            return True

        return True
