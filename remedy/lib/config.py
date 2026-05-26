"""
MIDICaptain Remedy - Configuration System

Handles loading and parsing TOML configuration files.
Includes a minimal TOML parser for CircuitPython (no tomllib available).
"""

import gc


class ConfigError(Exception):
    """Configuration parsing or validation error."""
    pass


# ═══════════════════════════════════════════════════════════════════════════════
# TOML PARSER
# ═══════════════════════════════════════════════════════════════════════════════

def _skip_whitespace(s, i):
    """Advance past whitespace."""
    while i < len(s) and s[i] in ' \t':
        i += 1
    return i


def _parse_string(s, i):
    """Parse a quoted string starting at position i. Returns (value, new_pos)."""
    quote = s[i]
    i += 1
    result = []
    while i < len(s) and s[i] != quote:
        if s[i] == '\\' and quote == '"':
            i += 1
            if i < len(s):
                esc = s[i]
                if esc == 'n':
                    result.append('\n')
                elif esc == 't':
                    result.append('\t')
                elif esc == '"':
                    result.append('"')
                elif esc == '\\':
                    result.append('\\')
                else:
                    result.append('\\')
                    result.append(esc)
        else:
            result.append(s[i])
        i += 1
    if i < len(s):
        i += 1  # skip closing quote
    return ''.join(result), i


def _parse_value(s, i):
    """Parse a TOML value starting at position i. Returns (value, new_pos)."""
    i = _skip_whitespace(s, i)
    if i >= len(s):
        return None, i

    ch = s[i]

    # String
    if ch in '"\'':
        return _parse_string(s, i)

    # Array
    if ch == '[':
        return _parse_array(s, i)

    # Inline table
    if ch == '{':
        return _parse_inline_table(s, i)

    # Bare value (bool, int, float)
    start = i
    while i < len(s) and s[i] not in ',]}# \t\n':
        i += 1
    token = s[start:i].strip()

    if token == 'true':
        return True, i
    if token == 'false':
        return False, i

    # Hex
    if token.startswith('0x') or token.startswith('0X'):
        return int(token, 16), i

    # Integer or float
    try:
        if '.' in token or 'e' in token.lower():
            return float(token), i
        return int(token), i
    except ValueError:
        return token, i


def _parse_array(s, i):
    """Parse a TOML array starting at '['. Returns (list, new_pos)."""
    i += 1  # skip '['
    result = []
    while i < len(s):
        i = _skip_whitespace(s, i)
        if i >= len(s):
            break
        if s[i] == ']':
            i += 1
            break
        if s[i] == ',':
            i += 1
            continue
        if s[i] == '#':
            # skip rest of line within array
            while i < len(s) and s[i] != '\n':
                i += 1
            continue
        if s[i] == '\n':
            i += 1
            continue
        val, i = _parse_value(s, i)
        result.append(val)
    return result, i


def _parse_inline_table(s, i):
    """Parse an inline table starting at '{'. Returns (dict, new_pos)."""
    i += 1  # skip '{'
    result = {}
    while i < len(s):
        i = _skip_whitespace(s, i)
        if i >= len(s):
            break
        if s[i] == '}':
            i += 1
            break
        if s[i] == ',':
            i += 1
            continue

        # Parse key
        key, i = _parse_key(s, i)
        i = _skip_whitespace(s, i)
        if i < len(s) and s[i] == '=':
            i += 1
        i = _skip_whitespace(s, i)

        # Parse value
        val, i = _parse_value(s, i)
        result[key] = val

    return result, i


def _parse_key(s, i):
    """Parse a key (bare or quoted). Returns (key_string, new_pos)."""
    i = _skip_whitespace(s, i)
    if i < len(s) and s[i] in '"\'':
        return _parse_string(s, i)

    start = i
    while i < len(s) and s[i] not in '= \t\n.':
        i += 1
    return s[start:i].strip(), i


def _strip_comment(line):
    """Remove trailing comment from a line (respecting strings)."""
    in_string = False
    quote_char = None
    for i, ch in enumerate(line):
        if not in_string and ch in '"\'':
            in_string = True
            quote_char = ch
        elif in_string and ch == quote_char:
            in_string = False
        elif ch == '#' and not in_string:
            return line[:i]
    return line


def _join_multiline(lines, start):
    """Join continuation lines for multi-line arrays/tables.
    Returns (joined_value_string, next_line_index)."""
    depth = 0
    in_string = False
    quote_char = None
    parts = []

    i = start
    while i < len(lines):
        line = _strip_comment(lines[i]).strip()
        i += 1

        for ch in line:
            if not in_string and ch in '"\'':
                in_string = True
                quote_char = ch
            elif in_string and ch == quote_char:
                in_string = False
            elif not in_string:
                if ch in '[{':
                    depth += 1
                elif ch in ']}':
                    depth -= 1

        parts.append(line)

        if depth <= 0:
            break

    return ' '.join(parts), i


def parse_toml(text_or_lines):
    """Parse TOML text (or list of lines) into a dictionary."""
    result = {}
    current_section = result
    if isinstance(text_or_lines, list):
        lines = text_or_lines
    else:
        lines = text_or_lines.split('\n')
    i = 0

    while i < len(lines):
        line = _strip_comment(lines[i]).strip()
        i += 1

        if not line:
            continue

        # Array of tables ([[name]])
        if line.startswith('[['):
            end = line.index(']]')
            section_name = line[2:end].strip()
            keys = section_name.split('.')
            parent = result
            for k in keys[:-1]:
                k = k.strip().strip('"').strip("'")
                if k not in parent:
                    parent[k] = {}
                parent = parent[k]
            last_key = keys[-1].strip().strip('"').strip("'")
            if last_key not in parent:
                parent[last_key] = []
            new_table = {}
            parent[last_key].append(new_table)
            current_section = new_table
            continue

        # Section header
        if line.startswith('['):
            section_name = line[1:line.index(']')].strip()
            keys = section_name.split('.')
            current_section = result
            for k in keys:
                k = k.strip().strip('"').strip("'")
                if k not in current_section:
                    current_section[k] = {}
                current_section = current_section[k]
            continue

        # Key-value pair
        eq_pos = -1
        in_str = False
        q_char = None
        for j, ch in enumerate(line):
            if not in_str and ch in '"\'':
                in_str = True
                q_char = ch
            elif in_str and ch == q_char:
                in_str = False
            elif ch == '=' and not in_str:
                eq_pos = j
                break

        if eq_pos < 0:
            continue

        raw_key = line[:eq_pos].strip()
        raw_val = line[eq_pos + 1:].strip()

        # Check if value spans multiple lines
        depth = 0
        in_str2 = False
        q2 = None
        for ch in raw_val:
            if not in_str2 and ch in '"\'':
                in_str2 = True
                q2 = ch
            elif in_str2 and ch == q2:
                in_str2 = False
            elif not in_str2:
                if ch in '[{':
                    depth += 1
                elif ch in ']}':
                    depth -= 1

        if depth > 0:
            rest, i = _join_multiline(lines, i)
            raw_val = raw_val + ' ' + rest

        # Parse the value
        val, _ = _parse_value(raw_val, 0)

        # Handle dotted keys
        key_parts = []
        ki = 0
        while ki < len(raw_key):
            ki = _skip_whitespace(raw_key, ki)
            if ki >= len(raw_key):
                break
            k, ki = _parse_key(raw_key, ki)
            key_parts.append(k)
            ki = _skip_whitespace(raw_key, ki)
            if ki < len(raw_key) and raw_key[ki] == '.':
                ki += 1

        target = current_section
        for k in key_parts[:-1]:
            if k not in target:
                target[k] = {}
            target = target[k]

        if key_parts:
            target[key_parts[-1]] = val

    return result


# ═══════════════════════════════════════════════════════════════════════════════
# CONFIG LOADER
# ═══════════════════════════════════════════════════════════════════════════════

def load_toml(filepath):
    """Load and parse a TOML file."""
    gc.collect()
    try:
        with open(filepath, 'r') as f:
            lines = f.readlines()
    except OSError as e:
        raise ConfigError(f"Cannot read config file: {filepath}") from e

    try:
        result = parse_toml(lines)
        del lines
        gc.collect()
        return result
    except Exception as e:
        raise ConfigError(f"TOML parse error in {filepath}: {e}") from e


def deep_merge(base, override):
    """Deep merge two dictionaries, with override taking precedence."""
    result = base.copy()
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def get_nested(data, path, default=None):
    """Get a nested value from a dictionary using dot notation."""
    keys = path.split('.') if isinstance(path, str) else path
    current = data
    for key in keys:
        if isinstance(current, dict) and key in current:
            current = current[key]
        else:
            return default
    return current


def set_nested(data, path, value):
    """Set a nested value in a dictionary using dot notation."""
    keys = path.split('.') if isinstance(path, str) else path
    current = data
    for key in keys[:-1]:
        if key not in current:
            current[key] = {}
        current = current[key]
    current[keys[-1]] = value


# ═══════════════════════════════════════════════════════════════════════════════
# CONFIG MANAGER
# ═══════════════════════════════════════════════════════════════════════════════

class Config:
    """
    Configuration manager for MIDICaptain Remedy.
    Handles loading global config, profiles, pages, and setlists.
    """

    DEFAULT_CONFIG_PATH = '/config'

    def __init__(self, config_path=None):
        self.config_path = config_path or self.DEFAULT_CONFIG_PATH
        self._global = {}
        self._profile = {}
        self._page = {}
        self._state = {}

    def load_global(self, filename='global.toml'):
        """Load global configuration."""
        filepath = f"{self.config_path}/{filename}"
        try:
            self._global = load_toml(filepath)
        except ConfigError:
            self._global = self._default_global()
        return self._global

    def load_profile(self, name):
        """Load a device profile."""
        filepath = f"{self.config_path}/profiles/{name}.toml"
        self._profile = load_toml(filepath)
        return self._profile

    def load_page(self, name):
        """Load a page configuration."""
        filepath = f"{self.config_path}/pages/{name}.toml"
        self._page = load_toml(filepath)
        return self._page

    def discover_pages(self):
        """Scan the pages directory and return sorted list of page names."""
        import os
        pages_dir = f"{self.config_path}/pages"
        pages = []
        try:
            for entry in os.listdir(pages_dir):
                if entry.endswith('.toml'):
                    pages.append(entry[:-5])
        except OSError:
            pass
        pages.sort()
        return pages

    def load_setlist(self, name):
        """Load a setlist."""
        filepath = f"{self.config_path}/setlists/{name}.toml"
        return load_toml(filepath)

    @property
    def midi_channel(self):
        return get_nested(self._global, 'midi.channel', 1)

    @property
    def display_brightness(self):
        return get_nested(self._global, 'display.brightness', 80)

    @property
    def led_brightness(self):
        return get_nested(self._global, 'leds.brightness', 50)

    @property
    def colors(self):
        return get_nested(self._global, 'colors', self._default_colors())

    def get_button_config(self, button_id):
        """Get configuration for a specific button."""
        page_buttons = get_nested(self._page, 'buttons', {})
        if button_id in page_buttons:
            return page_buttons[button_id]
        profile_defaults = get_nested(self._profile, 'defaults.buttons', {})
        if button_id in profile_defaults:
            return profile_defaults[button_id]
        return None

    def get_encoder_config(self):
        page_encoder = get_nested(self._page, 'encoder', {})
        if page_encoder:
            return page_encoder
        return get_nested(self._profile, 'defaults.encoder', {})

    def get_expression_config(self, pedal_id):
        page_exp = get_nested(self._page, f'expression.pedal{pedal_id}', {})
        if page_exp:
            return page_exp
        return get_nested(self._profile, f'defaults.expression.pedal{pedal_id}', {})

    def get_sysex_param(self, param_name):
        return get_nested(self._profile, f'sysex.parameters.{param_name}', None)

    def _default_global(self):
        return {
            'midi': {'channel': 1, 'usb_enabled': True, 'din_enabled': True},
            'display': {'brightness': 80, 'rotation': 180},
            'leds': {'brightness': 50, 'idle_brightness': 20},
            'colors': self._default_colors(),
        }

    def _default_colors(self):
        return {
            'off': [0, 0, 0],
            'red': [255, 0, 0],
            'green': [0, 255, 0],
            'blue': [0, 0, 255],
            'yellow': [255, 255, 0],
            'amber': [255, 128, 0],
            'purple': [128, 0, 255],
            'cyan': [0, 255, 255],
            'white': [255, 255, 255],
        }
