"""
MIDICaptain Remedy - Main Entry Point

Configuration-driven MIDI controller firmware for Paint Audio MIDI Captain.
"""

import time
import gc

from lib.config import Config, ConfigError
from lib.hardware import Hardware
from lib.midi import MidiInterface
from lib.display import DisplayManager
from lib.tuner import TunerController
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

        # State
        self._running = True
        self._current_page = None
        self._pages = []  # List of available page names
        self._state = {}  # Runtime state storage

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
            self.display.init()
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

        # Initialize tuner
        print("Initializing tuner...")
        tuner_config = self.config._global.get('tuner', {})
        self.tuner = TunerController(
            midi_interface=self.midi,
            display_manager=self.display,
            config=tuner_config
        )
        if self.display:
            font_large = self.display.load_font('xlarge')
            self.tuner.init_display(font_large=font_large)
        print("  Tuner initialized")

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

        # Show startup LED pattern
        self._startup_leds()

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

        # Load default page
        page_name = startup.get('page', 'default')
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
        """Update LEDs based on current page configuration."""
        colors = self.config.colors

        for button_name in ['1', '2', '3', '4', 'A', 'B', 'C', 'D', 'up', 'down']:
            button_config = self.config.get_button_config(button_name)

            if button_config:
                color_name = button_config.get('color', 'white')
                color = colors.get(color_name, [128, 128, 128])

                # Dim for idle state
                idle_brightness = self.config._global.get('leds', {}).get('idle_brightness', 20) / 100.0
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

            # Flash LED on press
            if action_type == 'press':
                colors = self.config.colors
                color_name = button_config.get('color', 'white')
                color = tuple(colors.get(color_name, [255, 255, 255]))
                self.hardware.leds.set_button_color(button_id, color)

        # Dim LED on release
        if action_type == 'release':
            self._update_button_led(button_id)

        event.handled = True

    def _update_button_led(self, button_id):
        """Update a single button's LED to its configured state."""
        button_config = self.config.get_button_config(button_id)
        colors = self.config.colors

        if button_config:
            color_name = button_config.get('color', 'white')
            color = colors.get(color_name, [128, 128, 128])
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

            # Could trigger LED updates, display updates, etc.

        elif midi_type == 'pc':
            program = event.data.get('program')
            self._state['current_program'] = program

        event.handled = True

    def _on_midi_message(self, msg_type, data):
        """Callback for incoming MIDI messages."""
        event = MidiEvent(msg_type, **data)
        self.events.queue(event)

    # ─────────────────────────────────────────────────────────────────────────
    # PAGE AND MODE MANAGEMENT
    # ─────────────────────────────────────────────────────────────────────────

    def _on_page_change(self, page_name, direction=None):
        """Handle page change requests."""
        if page_name:
            try:
                self.config.load_page(page_name)
                self._current_page = page_name
                self._update_leds()
            except ConfigError as e:
                print(f"Page load error: {e}")
        elif direction:
            # Navigate pages
            # For now, just print - need page list implementation
            print(f"Page nav: {direction}")

    def _toggle_tuner(self):
        """Toggle tuner mode."""
        self.tuner.toggle()

        if self.tuner.state.active:
            print("Tuner mode ON")
        else:
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
        import sys
        sys.print_exception(e)
    finally:
        if app:
            app.deinit()
        print("\nGoodbye!")


# Run if executed directly
if __name__ == '__main__':
    main()
