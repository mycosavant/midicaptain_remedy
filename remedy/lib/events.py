"""
MIDICaptain Remedy - Event System

Simple event dispatcher for decoupling input handling from actions.
"""


class Event:
    """Base event class."""

    def __init__(self, event_type, **kwargs):
        self.type = event_type
        self.data = kwargs
        self.handled = False

    def __repr__(self):
        return f"Event({self.type}, {self.data})"


# ═══════════════════════════════════════════════════════════════════════════════
# EVENT TYPES
# ═══════════════════════════════════════════════════════════════════════════════

class ButtonEvent(Event):
    """Button press/release event."""

    PRESS = 'press'
    RELEASE = 'release'
    LONG_PRESS = 'long_press'
    DOUBLE_TAP = 'double_tap'

    def __init__(self, button_id, action):
        super().__init__('button', button_id=button_id, action=action)

    @property
    def button_id(self):
        return self.data['button_id']

    @property
    def action(self):
        return self.data['action']


class EncoderEvent(Event):
    """Encoder rotation event."""

    def __init__(self, delta):
        super().__init__('encoder', delta=delta)

    @property
    def delta(self):
        return self.data['delta']


class ExpressionEvent(Event):
    """Expression pedal value change."""

    def __init__(self, pedal_id, value):
        super().__init__('expression', pedal_id=pedal_id, value=value)

    @property
    def pedal_id(self):
        return self.data['pedal_id']

    @property
    def value(self):
        return self.data['value']


class MidiEvent(Event):
    """Incoming MIDI message."""

    CC = 'cc'
    PC = 'pc'
    NOTE_ON = 'note_on'
    NOTE_OFF = 'note_off'
    SYSEX = 'sysex'
    CLOCK = 'clock'

    def __init__(self, midi_type, **kwargs):
        super().__init__('midi', midi_type=midi_type, **kwargs)

    @property
    def midi_type(self):
        return self.data['midi_type']


class SystemEvent(Event):
    """System events (page change, profile load, etc.)."""

    PAGE_CHANGE = 'page_change'
    PROFILE_LOAD = 'profile_load'
    SETLIST_CHANGE = 'setlist_change'
    TUNER_TOGGLE = 'tuner_toggle'

    def __init__(self, system_type, **kwargs):
        super().__init__('system', system_type=system_type, **kwargs)


# ═══════════════════════════════════════════════════════════════════════════════
# EVENT DISPATCHER
# ═══════════════════════════════════════════════════════════════════════════════

class EventDispatcher:
    """
    Central event dispatcher.

    Handlers are called in order of registration.
    Handlers can mark events as handled to stop propagation.
    """

    def __init__(self):
        self._handlers = {}  # event_type -> list of handlers
        self._queue = []     # Pending events

    def register(self, event_type, handler, priority=0):
        """
        Register a handler for an event type.

        Args:
            event_type: String event type ('button', 'midi', etc.)
            handler: Callable that takes an Event and returns None
            priority: Higher priority handlers run first
        """
        if event_type not in self._handlers:
            self._handlers[event_type] = []

        self._handlers[event_type].append((priority, handler))
        # Sort by priority (descending)
        self._handlers[event_type].sort(key=lambda x: -x[0])

    def unregister(self, event_type, handler):
        """Remove a handler."""
        if event_type in self._handlers:
            self._handlers[event_type] = [
                (p, h) for p, h in self._handlers[event_type]
                if h != handler
            ]

    def emit(self, event):
        """
        Dispatch an event to registered handlers.

        Returns True if any handler processed the event.
        """
        handlers = self._handlers.get(event.type, [])

        for priority, handler in handlers:
            try:
                handler(event)
                if event.handled:
                    return True
            except Exception as e:
                print(f"Error in event handler: {e}")

        return event.handled

    def queue(self, event):
        """Add event to queue for later processing."""
        self._queue.append(event)

    def process_queue(self):
        """Process all queued events."""
        while self._queue:
            event = self._queue.pop(0)
            self.emit(event)


# ═══════════════════════════════════════════════════════════════════════════════
# ACTION SYSTEM
# ═══════════════════════════════════════════════════════════════════════════════

class Action:
    """
    Base class for executable actions.

    Actions are triggered by events and can send MIDI, change pages, etc.
    """

    def __init__(self, action_type, **kwargs):
        self.type = action_type
        self.params = kwargs

    def execute(self, context):
        """
        Execute the action.

        Args:
            context: ActionContext with access to MIDI, state, etc.
        """
        raise NotImplementedError

    @classmethod
    def from_config(cls, config):
        """Create an Action from config dict."""
        action_type = config.get('type', 'none')

        if action_type == 'midi_cc':
            return MidiCCAction(
                cc=config.get('cc', 0),
                value=config.get('value', 127),
                channel=config.get('channel')
            )
        elif action_type == 'midi_pc':
            return MidiPCAction(
                program=config.get('program', 0),
                channel=config.get('channel')
            )
        elif action_type == 'page_change':
            return PageChangeAction(page=config.get('page'))
        elif action_type == 'page_next':
            return PageNavAction(direction='next')
        elif action_type == 'page_prev':
            return PageNavAction(direction='prev')
        elif action_type == 'sysex':
            return SysExAction(
                profile=config.get('profile'),
                param=config.get('param'),
                value=config.get('value'),
                address=config.get('address'),
                data=config.get('data')
            )
        elif action_type == 'tuner_toggle':
            return TunerToggleAction()
        else:
            return NoopAction()


class NoopAction(Action):
    """Action that does nothing."""

    def __init__(self):
        super().__init__('none')

    def execute(self, context):
        pass


class MidiCCAction(Action):
    """Send MIDI Control Change."""

    def __init__(self, cc, value=127, channel=None):
        super().__init__('midi_cc', cc=cc, value=value, channel=channel)

    def execute(self, context):
        cc = self.params['cc']
        value = self.params['value']
        channel = self.params['channel'] or context.midi_channel

        # Handle 'toggle' value
        if value == 'toggle':
            # Get current state and invert
            current = context.get_state(f'cc.{cc}', 0)
            value = 0 if current > 63 else 127
            context.set_state(f'cc.{cc}', value)

        context.midi.send_cc(channel, cc, value)


class MidiPCAction(Action):
    """Send MIDI Program Change."""

    def __init__(self, program, channel=None):
        super().__init__('midi_pc', program=program, channel=channel)

    def execute(self, context):
        program = self.params['program']
        channel = self.params['channel'] or context.midi_channel
        context.midi.send_pc(channel, program)


class SysExAction(Action):
    """Send SysEx message."""

    def __init__(self, profile=None, param=None, value=None, address=None, data=None):
        super().__init__('sysex',
                        profile=profile,
                        param=param,
                        value=value,
                        address=address,
                        data=data)

    def execute(self, context):
        # If using named parameter from profile
        if self.params.get('param'):
            param_def = context.config.get_sysex_param(self.params['param'])
            if param_def:
                address = param_def.get('address')
                value = self.params.get('value')
                context.midi.send_sysex_param(address, value)
        # If using raw address/data
        elif self.params.get('address'):
            context.midi.send_sysex(
                self.params['address'],
                self.params.get('data', [])
            )


class PageChangeAction(Action):
    """Change to a specific page."""

    def __init__(self, page):
        super().__init__('page_change', page=page)

    def execute(self, context):
        context.change_page(self.params['page'])


class PageNavAction(Action):
    """Navigate pages forward/backward."""

    def __init__(self, direction):
        super().__init__('page_nav', direction=direction)

    def execute(self, context):
        context.navigate_page(self.params['direction'])


class TunerToggleAction(Action):
    """Toggle tuner mode."""

    def __init__(self):
        super().__init__('tuner_toggle')

    def execute(self, context):
        context.toggle_tuner()


# ═══════════════════════════════════════════════════════════════════════════════
# ACTION CONTEXT
# ═══════════════════════════════════════════════════════════════════════════════

class ActionContext:
    """
    Context passed to actions providing access to system resources.
    """

    def __init__(self, midi, config, state_manager):
        self.midi = midi
        self.config = config
        self._state = state_manager
        self._page_callback = None
        self._tuner_callback = None

    @property
    def midi_channel(self):
        """Get default MIDI channel."""
        return self.config.midi_channel

    def get_state(self, key, default=None):
        """Get state value."""
        return self._state.get(key, default)

    def set_state(self, key, value):
        """Set state value."""
        self._state[key] = value

    def change_page(self, page_name):
        """Request page change."""
        if self._page_callback:
            self._page_callback(page_name)

    def navigate_page(self, direction):
        """Navigate pages."""
        if self._page_callback:
            self._page_callback(None, direction)

    def toggle_tuner(self):
        """Toggle tuner mode."""
        if self._tuner_callback:
            self._tuner_callback()

    def set_page_callback(self, callback):
        """Set page change callback."""
        self._page_callback = callback

    def set_tuner_callback(self, callback):
        """Set tuner toggle callback."""
        self._tuner_callback = callback
