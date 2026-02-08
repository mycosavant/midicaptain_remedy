"""
MIDICaptain Remedy - Main Entry Point

Configuration-driven MIDI controller firmware for Paint Audio MIDI Captain.
"""

import time
import gc
import displayio

from lib.config import Config, ConfigError
from lib.hardware import Hardware
from lib.midi import MidiInterface
from lib.display import DisplayManager
from lib.tuner import TunerController
from lib.menu import MenuSystem
from lib.events import (
    EventDispatcher,
    ButtonEvent,
    EncoderEvent,
    ExpressionEvent,
    MidiEvent,
    Action,
    ActionContext
)


# ═══════════════════════════════════════════════════════════════════════════════
# APPLICATION
# ═══════════════════════════════════════════════════════════════════════════════

class MidiCaptainApp:
    """
    Main application class for MIDICaptain Remedy.
    """

    VERSION = "0.1.0"

    def __init__(self):
        print(f"\n=== MIDICaptain Remedy v{self.VERSION} ===\n")

        # Release any display/SPI resources from previous run.
        # On RP2040 soft resets, SPI pin state persists (CP issue #4838).
        # This must happen before ANY other initialization.
        displayio.release_displays()
        gc.collect()

        # State
        self._running = True
        self._current_page = None
        self._pages = []  # List of available page names
        self._state = {}  # Runtime state storage
        self._setlist = None  # Active setlist data
        self._song_index = -1  # Current song index (-1 = no setlist)

        # Load configuration
        print("Loading configuration...")
        self.config = Config()
        try:
            self.config.load_global()
            print("  Global config loaded")
        except ConfigError as e:
            print(f"  Using defaults: {e}")

        # Initialize hardware
        print("Initializing hardware...")
        self.hardware = Hardware(self.config)
        print("  Hardware initialized")

        # Initialize display
        print("Initializing display...")
        self.display = DisplayManager()
        try:
            display_brightness = self.config.display_brightness / 100.0
            self.display.init(brightness=display_brightness)
            print("  Display initialized")
        except Exception as e:
            print(f"  Display init failed: {e}")
            self.display = None

        # Initialize MIDI
        print("Initializing MIDI...")
        self.midi = MidiInterface(
            usb_enabled=self.config._global.get('midi', {}).get('usb_enabled', True),
            din_enabled=self.config._global.get('midi', {}).get('din_enabled', True),
            default_channel=self.config.midi_channel
        )
        self.midi.set_message_callback(self._on_midi_message)
        print("  MIDI initialized")

        # Initialize tuner (display init deferred to first activation to save RAM)
        print("Initializing tuner...")
        tuner_config = self.config._global.get('tuner', {})
        self.tuner = TunerController(
            midi_interface=self.midi,
            display_manager=self.display,
            config=tuner_config
        )
        self._tuner_display_ready = False
        print("  Tuner initialized")

        # Settings menu
        self.menu = MenuSystem(self.display, self.config, self.hardware)

        # Load saved expression pedal calibration from NVM
        MenuSystem.load_calibration_from_nvm(self.hardware)

        # Event dispatcher
        self.events = EventDispatcher()

        # Action context
        self.context = ActionContext(self.midi, self.config, self._state)
        self.context.set_page_callback(self._on_page_change)
        self.context.set_tuner_callback(self._toggle_tuner)

        # Register event handlers
        self._setup_event_handlers()

        # Load default profile and page
        self._load_startup_config()

        # Build home screen
        self._init_home_screen()

        # Show startup LED pattern
        self._startup_leds()

        # Query device state if supported by profile
        self._query_device_state()

        print("\nReady!\n")

    def _setup_event_handlers(self):
        """Register event handlers."""
        self.events.register('button', self._handle_button_event)
        self.events.register('encoder', self._handle_encoder_event)
        self.events.register('expression', self._handle_expression_event)
        self.events.register('midi', self._handle_midi_event)

    def _load_startup_config(self):
        """Load startup profile and page from config."""
        startup = self.config._global.get('startup', {})

        # Load profile
        profile_name = startup.get('profile', 'generic_cc')
        try:
            self.config.load_profile(profile_name)
            print(f"  Profile loaded: {profile_name}")
        except ConfigError as e:
            print(f"  Profile not found: {e}")

        # Discover available pages
        self._pages = self.config.discover_pages()
        print(f"  Pages found: {self._pages}")

        # Load default page (may be overridden by setlist)
        page_name = startup.get('page', 'default')

        # Load setlist if configured
        setlist_name = startup.get('setlist', '')
        if setlist_name:
            try:
                self._setlist = self.config.load_setlist(setlist_name)
                sl_page = self._setlist.get('setlist', {}).get('page')
                if sl_page:
                    page_name = sl_page
                songs = self._setlist.get('songs', [])
                print(f"  Setlist loaded: {setlist_name} ({len(songs)} songs)")
            except ConfigError as e:
                print(f"  Setlist not found: {e}")

        try:
            self.config.load_page(page_name)
            self._current_page = page_name
            print(f"  Page loaded: {page_name}")
        except ConfigError as e:
            print(f"  Page not found: {e}")

        # Update LEDs for current page
        self._update_leds()

    def _startup_leds(self):
        """Show startup LED animation."""
        colors = self.config.colors

        # Quick sweep animation
        for button_name in ['1', '2', '3', '4', 'A', 'B', 'C', 'D']:
            self.hardware.leds.set_button_color(button_name, tuple(colors.get('cyan', [0, 255, 255])))
            time.sleep(0.05)

        time.sleep(0.2)

        # Set to page colors
        self._update_leds()

    def _update_leds(self):
        """Update all LEDs based on current page config and toggle state."""
        colors = self.config.colors
        idle_brightness = self.config._global.get('leds', {}).get('idle_brightness', 20) / 100.0

        for button_name in ['1', '2', '3', '4', 'A', 'B', 'C', 'D', 'up', 'down']:
            button_config = self.config.get_button_config(button_name)

            if button_config:
                color_name = button_config.get('color', 'white')
                color = colors.get(color_name, [128, 128, 128])

                is_on = self._state.get(f'toggle.{button_name}', False)
                if is_on:
                    self.hardware.leds.set_button_color(button_name, tuple(color), show=False)
                else:
                    dimmed = tuple(int(c * idle_brightness) for c in color)
                    self.hardware.leds.set_button_color(button_name, dimmed, show=False)
            else:
                self.hardware.leds.set_button_color(button_name, (0, 0, 0), show=False)

        self.hardware.leds.show()

    # ─────────────────────────────────────────────────────────────────────────
    # EVENT HANDLERS
    # ─────────────────────────────────────────────────────────────────────────

    def _handle_button_event(self, event):
        """Handle button press/release events."""
        button_id = event.button_id
        action_type = event.action

        # Encoder button: toggle menu
        if button_id == 'encoder':
            if action_type == 'long_press':
                if self.menu.active:
                    self.menu.deactivate()
                else:
                    self.menu.activate()
                event.handled = True
                return
            elif action_type == 'press' and self.menu.active:
                self.menu.handle_button()
                event.handled = True
                return

        # During calibration, any button advances the wizard
        if self.menu.active and self.menu._cal_step > 0 and action_type == 'press':
            self.menu.handle_calibration_button()
            event.handled = True
            return

        # Menu is active — ignore normal button actions
        if self.menu.active:
            event.handled = True
            return

        # Setlist mode: up/down short-press navigates songs
        if self._setlist and action_type == 'press':
            if button_id == 'up':
                self._navigate_setlist('next')
                event.handled = True
                return
            elif button_id == 'down':
                self._navigate_setlist('prev')
                event.handled = True
                return

        # Get button configuration
        button_config = self.config.get_button_config(button_id)
        if not button_config:
            return

        # Get the appropriate action config
        action_key = f'on_{action_type}'  # 'on_press', 'on_release', 'on_long_press'
        action_config = button_config.get(action_key)

        if action_config:
            action = Action.from_config(action_config)
            action.execute(self.context)

            # Track toggle state for CC toggles
            if action_type == 'press' and action_config.get('value') == 'toggle':
                state_key = f'toggle.{button_id}'
                toggled = not self._state.get(state_key, False)
                self._state[state_key] = toggled

            # Update LED to reflect toggle state or flash on press
            if action_type == 'press':
                self._update_button_led(button_id)

        # Restore LED on release (non-toggle buttons dim back)
        if action_type == 'release':
            self._update_button_led(button_id)

        event.handled = True

    def _update_button_led(self, button_id):
        """Update a single button's LED based on config and toggle state."""
        button_config = self.config.get_button_config(button_id)
        colors = self.config.colors

        if button_config:
            color_name = button_config.get('color', 'white')
            color = colors.get(color_name, [128, 128, 128])

            # Check if this button has a toggle that's currently ON
            is_on = self._state.get(f'toggle.{button_id}', False)

            if is_on:
                # Full brightness for active toggles
                self.hardware.leds.set_button_color(button_id, tuple(color))
            else:
                # Dimmed for idle / toggled-off
                idle_brightness = self.config._global.get('leds', {}).get('idle_brightness', 20) / 100.0
                dimmed = tuple(int(c * idle_brightness) for c in color)
                self.hardware.leds.set_button_color(button_id, dimmed)
        else:
            self.hardware.leds.set_button_color(button_id, (0, 0, 0))

    def _handle_encoder_event(self, event):
        """Handle encoder rotation."""
        delta = event.delta
        if delta == 0:
            return

        # Route to menu if active
        if self.menu.active:
            self.menu.handle_encoder(delta)
            event.handled = True
            return

        encoder_config = self.config.get_encoder_config()
        if not encoder_config:
            return

        bind = encoder_config.get('bind')
        if bind:
            # Get current value from state
            state_key = f'encoder.{bind}'
            current = self._state.get(state_key, 64)

            # Update value
            new_value = max(0, min(127, current + delta))
            self._state[state_key] = new_value

            # Send MIDI
            if bind.startswith('sysex:'):
                param_name = bind[6:]
                param = self.config.get_sysex_param(param_name)
                if param:
                    # Scale from 0-127 to param range
                    param_min = param.get('min', 0)
                    param_max = param.get('max', 100)
                    scaled = int(param_min + (new_value / 127) * (param_max - param_min))
                    self.midi.send_sysex_param(param['address'], [scaled])
            else:
                # Assume it's a CC number or action
                fallback = encoder_config.get('fallback', {})
                cc = fallback.get('cc', 7)
                self.midi.send_cc(self.config.midi_channel, cc, new_value)

        event.handled = True

    def _handle_expression_event(self, event):
        """Handle expression pedal changes."""
        pedal_id = event.pedal_id
        value = event.value

        exp_config = self.config.get_expression_config(pedal_id)
        if not exp_config:
            return

        bind = exp_config.get('bind')
        if bind:
            if bind.startswith('sysex:'):
                param_name = bind[6:]
                param = self.config.get_sysex_param(param_name)
                if param:
                    # Scale value to param range
                    param_min = param.get('min', 0)
                    param_max = param.get('max', 100)
                    scaled = int(param_min + (value / 127) * (param_max - param_min))
                    self.midi.send_sysex_param(param['address'], [scaled])
            elif bind.startswith('midi_cc'):
                cc = exp_config.get('cc', 1)
                self.midi.send_cc(self.config.midi_channel, cc, value)

        event.handled = True

    def _handle_midi_event(self, event):
        """Handle incoming MIDI messages."""
        midi_type = event.midi_type

        # First, let tuner handle relevant messages
        if self.tuner.process_midi_message(midi_type, event.data):
            event.handled = True
            return

        if midi_type == 'cc':
            cc = event.data.get('cc')
            value = event.data.get('value')

            # Update state for bidirectional feedback
            self._state[f'cc.{cc}'] = value

            # Update toggle LED if this CC maps to a button
            self._sync_cc_to_toggle(cc, value)

        elif midi_type == 'pc':
            program = event.data.get('program')
            self._state['current_program'] = program

        elif midi_type == 'sysex':
            self._handle_sysex_response(event.data)

        event.handled = True

    def _on_midi_message(self, msg_type, data):
        """Callback for incoming MIDI messages."""
        event = MidiEvent(msg_type, **data)
        self.events.queue(event)

    # ─────────────────────────────────────────────────────────────────────────
    # HOME SCREEN
    # ─────────────────────────────────────────────────────────────────────────

    def _init_home_screen(self):
        """Build a lightweight text-only home screen."""
        if not self.display or not self.display._display:
            return

        try:
            import terminalio
            from adafruit_display_text import label as text_label
        except ImportError:
            return

        font = terminalio.FONT
        layer = self.display.create_layer('home')

        # Page name header (centered, scale 2)
        page_name = (self._current_page or "Default").upper()
        self._home_title = text_label.Label(
            font, text=page_name,
            color=0xFFFFFF,
            anchor_point=(0.5, 0),
            anchored_position=(120, 4),
            scale=2
        )
        layer.append(self._home_title)

        # Button assignments: 2 columns matching physical layout
        # Left column: top row (1-4), Right column: bottom row (A-D)
        # Format: "ID:LABEL" truncated to fit columns at scale 2
        self._home_labels = {}
        colors = self.config.colors

        button_rows = [('1', 'A'), ('2', 'B'), ('3', 'C'), ('4', 'D')]
        y_start = 50
        row_spacing = 30

        for row_idx, (left_id, right_id) in enumerate(button_rows):
            y = y_start + row_idx * row_spacing
            for btn_id, x_pos in ((left_id, 4), (right_id, 128)):
                btn_cfg = self.config.get_button_config(btn_id)
                func_text = (btn_cfg.get('label', '') if btn_cfg else '')[:6]
                color_name = btn_cfg.get('color', 'white') if btn_cfg else 'white'
                rgb = colors.get(color_name, [128, 128, 128])

                lbl = text_label.Label(
                    font,
                    text=f"{btn_id}:{func_text}",
                    color=self._rgb_pack(rgb[0], rgb[1], rgb[2]),
                    anchor_point=(0, 0.5),
                    anchored_position=(x_pos, y),
                    scale=2
                )
                layer.append(lbl)
                self._home_labels[btn_id] = lbl

        # Footer
        layer.append(text_label.Label(
            font, text=f"Remedy v{self.VERSION}",
            color=0x555555,
            anchor_point=(0.5, 1.0),
            anchored_position=(120, 236),
            scale=1
        ))

        print("  Home screen ready")

    @staticmethod
    def _rgb_pack(r, g, b):
        """Pack RGB bytes into a 24-bit integer."""
        return (max(0, min(255, r)) << 16) | (max(0, min(255, g)) << 8) | max(0, min(255, b))

    def _refresh_home_screen(self):
        """Update home screen content after page change."""
        if not hasattr(self, '_home_title'):
            return

        gc.collect()
        try:
            self._home_title.text = (self._current_page or "Default").upper()

            colors = self.config.colors
            for btn_id, lbl in self._home_labels.items():
                btn_cfg = self.config.get_button_config(btn_id)
                func_text = (btn_cfg.get('label', '') if btn_cfg else '')[:6]
                lbl.text = f"{btn_id}:{func_text}"
                color_name = btn_cfg.get('color', 'white') if btn_cfg else 'white'
                rgb = colors.get(color_name, [128, 128, 128])
                lbl.color = self._rgb_pack(rgb[0], rgb[1], rgb[2])
        except MemoryError:
            pass  # Labels keep previous text; LEDs still update correctly

    # ─────────────────────────────────────────────────────────────────────────
    # DEVICE SYNC
    # ─────────────────────────────────────────────────────────────────────────

    def _build_cc_button_map(self):
        """Build a reverse map: CC number → button_id for toggle buttons."""
        self._cc_to_button = {}
        for btn_id in ['1', '2', '3', '4', 'A', 'B', 'C', 'D', 'up', 'down']:
            btn_cfg = self.config.get_button_config(btn_id)
            if btn_cfg:
                on_press = btn_cfg.get('on_press', {})
                if on_press.get('type') == 'midi_cc' and on_press.get('value') == 'toggle':
                    cc = on_press.get('cc')
                    if cc is not None:
                        self._cc_to_button[cc] = btn_id

    def _sync_cc_to_toggle(self, cc, value):
        """Update button toggle state when matching CC is received from device."""
        if not hasattr(self, '_cc_to_button'):
            return
        btn_id = self._cc_to_button.get(cc)
        if btn_id is not None:
            self._state[f'toggle.{btn_id}'] = (value > 63)
            self._update_button_led(btn_id)

    def _handle_sysex_response(self, data):
        """Parse SysEx response and update toggle state for known parameters."""
        sysex_data = data.get('data')
        if not sysex_data or len(sysex_data) < 10:
            return

        # Roland DT1 response: [device_id, model_id(4), 0x12, addr(4), data..., checksum]
        # After manufacturer_id stripping, we get the inner payload
        raw = list(sysex_data)

        # Look for set operation (0x12) - this is what the amp sends back
        # Format varies but address starts after model_id + operation byte
        # We need to find the address and match it to known parameters
        params = self.config._profile.get('sysex', {}).get('parameters', {})
        if not params:
            return

        # Try to extract address (bytes 5-8 after manufacturer in typical Roland response)
        # The exact offset depends on the message format
        # For Katana: [dev_id, model(4), op, addr(4), data..., checksum]
        if len(raw) >= 10:
            op = raw[5] if len(raw) > 5 else 0
            if op == 0x12:  # DT1 (data set)
                addr = raw[6:10]
                param_data = raw[10:-1]  # Exclude checksum

                # Match address to known bool parameters with cc_alias
                for param_name, param_cfg in params.items():
                    if param_cfg.get('type') == 'bool' and param_cfg.get('address') == addr:
                        cc_alias = param_cfg.get('cc_alias')
                        if cc_alias is not None and param_data:
                            self._sync_cc_to_toggle(cc_alias, param_data[0] * 127)

    def _query_device_state(self):
        """Query device for current effect switch states on startup."""
        startup = self.config._global.get('startup', {})
        if not startup.get('query_device', False):
            return

        sysex_cfg = self.config._profile.get('sysex', {})
        if not sysex_cfg.get('enabled', False):
            return

        # Build CC→button map for response handling
        self._build_cc_button_map()

        # Enter editor mode if required
        editor = sysex_cfg.get('editor_mode', {})
        if editor:
            enter_cfg = editor.get('enter', {})
            if enter_cfg:
                self.midi.send_sysex_param(
                    enter_cfg.get('address', []),
                    enter_cfg.get('data', [])
                )
                settle = editor.get('settle_time_ms', 100)
                time.sleep(settle / 1000.0)

        # Query all bool parameters (effect switches)
        params = sysex_cfg.get('parameters', {})
        for param_name, param_cfg in params.items():
            if param_cfg.get('type') == 'bool' and param_cfg.get('cc_alias'):
                addr = param_cfg.get('address')
                if addr:
                    self.midi.query_sysex_param(addr, length=1)
                    time.sleep(0.02)  # Small delay between queries

        print("  Device state queried")

    # ─────────────────────────────────────────────────────────────────────────
    # PAGE AND MODE MANAGEMENT
    # ─────────────────────────────────────────────────────────────────────────

    def _on_page_change(self, page_name, direction=None):
        """Handle page change requests."""
        if page_name:
            target = page_name
        elif direction and self._pages:
            # Cycle through discovered pages
            try:
                idx = self._pages.index(self._current_page)
            except ValueError:
                idx = 0
            if direction == 'next':
                idx = (idx + 1) % len(self._pages)
            elif direction == 'prev':
                idx = (idx - 1) % len(self._pages)
            target = self._pages[idx]
        else:
            return

        if target == self._current_page and page_name is None:
            return  # Already on this page (single-page list)

        # Clear toggle states when switching pages
        for key in list(self._state.keys()):
            if key.startswith('toggle.'):
                del self._state[key]

        # Free old page data and reclaim memory before loading new page
        self.config._page = {}
        gc.collect()

        try:
            self.config.load_page(target)
            self._current_page = target
            self._build_cc_button_map()
            self._update_leds()
            self._refresh_home_screen()
            print(f"Page: {target}")
        except ConfigError as e:
            print(f"Page load error: {e}")

    def _navigate_setlist(self, direction):
        """Navigate to next/previous song in setlist."""
        if not self._setlist:
            return

        songs = self._setlist.get('songs', [])
        if not songs:
            return

        if direction == 'next':
            self._song_index = min(self._song_index + 1, len(songs) - 1)
        elif direction == 'prev':
            self._song_index = max(self._song_index - 1, 0)

        song = songs[self._song_index]
        song_name = song.get('name', f'Song {self._song_index + 1}')
        print(f"Song: {song_name}")

        # Execute on_enter actions
        on_enter = song.get('on_enter', [])
        for action_cfg in on_enter:
            action = Action.from_config(action_cfg)
            action.execute(self.context)

        # Update home screen title to show song name
        if hasattr(self, '_home_title'):
            self._home_title.text = song_name.upper()

    def _toggle_tuner(self):
        """Toggle tuner mode."""
        # Lazy-init tuner display on first activation
        if not self._tuner_display_ready:
            self._tuner_display_ready = True  # Set early to prevent retries
            if self.display:
                gc.collect()
                try:
                    font_large = self.display.load_font('xlarge')
                except MemoryError:
                    font_large = None
                try:
                    self.tuner.init_display(font_large=font_large)
                except MemoryError:
                    print("  Tuner: not enough RAM")
                    return

        self.tuner.toggle()

        if self.tuner.state.active:
            # Hide home, show tuner
            if self.display:
                home = self.display.get_layer('home')
                if home:
                    home.hidden = True
            print("Tuner mode ON")
        else:
            # Show home, tuner layer hidden by TunerController.toggle()
            if self.display:
                home = self.display.get_layer('home')
                if home:
                    home.hidden = False
            print("Tuner mode OFF")
            self._update_leds()

    # ─────────────────────────────────────────────────────────────────────────
    # MAIN LOOP
    # ─────────────────────────────────────────────────────────────────────────

    def run(self):
        """Main application loop."""
        last_gc = time.monotonic()
        last_display = time.monotonic()

        while self._running:
            # Update hardware and get events
            hw_events = self.hardware.update()

            # Convert hardware events to Event objects
            for button_name, action in hw_events['buttons']:
                event = ButtonEvent(button_name, action)
                self.events.emit(event)

            encoder_delta = hw_events['encoder']
            if encoder_delta:
                event = EncoderEvent(encoder_delta)
                self.events.emit(event)

            for pedal_id, value in hw_events['expression'].items():
                event = ExpressionEvent(pedal_id, value)
                self.events.emit(event)

            # Check for incoming MIDI
            self.midi.receive()

            # Process queued events
            self.events.process_queue()

            # Update display (throttled to ~30fps)
            now = time.monotonic()
            if now - last_display > 0.033:
                # Update tuner display if active
                self.tuner.update()

                # Update other display elements
                if self.display:
                    self.display.update()

                last_display = now

            # Periodic garbage collection (every 5 seconds)
            if now - last_gc > 5:
                gc.collect()
                last_gc = now

            # Small delay to prevent CPU hogging
            time.sleep(0.001)

    def stop(self):
        """Stop the application."""
        self._running = False

    def deinit(self):
        """Clean up resources."""
        self.hardware.deinit()
        self.midi.deinit()
        if self.display:
            self.display.deinit()


# ═══════════════════════════════════════════════════════════════════════════════
# ENTRY POINT
# ═══════════════════════════════════════════════════════════════════════════════

def main():
    """Application entry point."""
    app = None

    try:
        app = MidiCaptainApp()
        app.run()
    except KeyboardInterrupt:
        print("\nShutdown requested")
    except Exception as e:
        print(f"\nError: {e}")
        import traceback
        traceback.print_exception(e)
    finally:
        if app:
            app.deinit()
        print("\nGoodbye!")


# Run if executed directly
if __name__ == '__main__':
    main()
