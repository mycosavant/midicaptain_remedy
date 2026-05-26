"""
MIDICaptain Remedy - Display Module

Optimized display management for the ST7789 240x240 TFT display.
Features:
- Lazy rendering (only update what changed)
- Object pooling (reuse display elements)
- Clean class-based design
- Configurable color themes
"""

import gc
import displayio
import fourwire
import busio
import pwmio
import terminalio

# CircuitPython imports (may not exist in dev environment)
try:
    from adafruit_st7789 import ST7789
    from adafruit_display_text import label
    from adafruit_bitmap_font import bitmap_font
    from adafruit_display_shapes.rect import Rect
except ImportError:
    pass

from . import pins


# ═══════════════════════════════════════════════════════════════════════════════
# COLOR MANAGEMENT
# ═══════════════════════════════════════════════════════════════════════════════

class ColorPalette:
    """
    Efficient color palette with lazy computation of dimmed variants.

    Colors are stored as RGB tuples. Dimmed/dark variants are computed
    on first access and cached.
    """

    # Base colors (full brightness)
    COLORS = {
        'black':          (0, 0, 0),
        'white':          (255, 255, 255),
        'red':            (255, 0, 0),
        'green':          (0, 255, 0),
        'blue':           (0, 0, 255),
        'yellow':         (255, 255, 0),
        'cyan':           (0, 255, 255),
        'magenta':        (255, 0, 255),
        'orange':         (255, 128, 0),
        'purple':         (128, 0, 255),
        'lime':           (128, 255, 0),
        'spring':         (0, 255, 128),
        'azure':          (0, 128, 255),
        'violet':         (128, 0, 255),
        'amber':          (255, 191, 0),
        'grey':           (128, 128, 128),
        'dark_red':       (128, 0, 0),
        'dark_green':     (0, 128, 0),
        'dark_blue':      (0, 0, 128),
        'dark_yellow':    (128, 128, 0),
        'dark_cyan':      (0, 128, 128),
        'dark_magenta':   (128, 0, 128),
    }

    def __init__(self):
        self._dim_cache = {}
        self._dark_cache = {}

    def get(self, name, default=None):
        """Get a color by name."""
        return self.COLORS.get(name, default or (128, 128, 128))

    def dim(self, color, factor=12):
        """
        Get a dimmed version of a color (for LED idle state).

        Args:
            color: RGB tuple or color name
            factor: Division factor (higher = dimmer)

        Returns:
            Dimmed RGB tuple
        """
        if isinstance(color, str):
            color = self.get(color)

        cache_key = (color, factor)
        if cache_key not in self._dim_cache:
            self._dim_cache[cache_key] = tuple(c // factor for c in color)
        return self._dim_cache[cache_key]

    def dark(self, color, factor=3):
        """
        Get a darkened version of a color (for display backgrounds).

        Args:
            color: RGB tuple or color name
            factor: Division factor (higher = darker)

        Returns:
            Darkened RGB tuple
        """
        if isinstance(color, str):
            color = self.get(color)

        cache_key = (color, factor)
        if cache_key not in self._dark_cache:
            self._dark_cache[cache_key] = tuple(c // factor for c in color)
        return self._dark_cache[cache_key]

    def to_displayio(self, color):
        """Convert RGB tuple to displayio color (24-bit integer)."""
        if isinstance(color, str):
            color = self.get(color)
        return (color[0] << 16) | (color[1] << 8) | color[2]


# Global palette instance
PALETTE = ColorPalette()


# ═══════════════════════════════════════════════════════════════════════════════
# DISPLAY ELEMENTS
# ═══════════════════════════════════════════════════════════════════════════════

class DisplayElement:
    """
    Base class for display elements.

    Implements dirty flag pattern for efficient updates.
    """

    def __init__(self, x, y, width, height):
        self.x = x
        self.y = y
        self.width = width
        self.height = height
        self._dirty = True
        self._visible = True
        self._group = None

    @property
    def dirty(self):
        return self._dirty

    def mark_dirty(self):
        """Mark this element for redraw."""
        self._dirty = True

    def clear_dirty(self):
        """Clear the dirty flag after rendering."""
        self._dirty = False

    @property
    def visible(self):
        return self._visible

    @visible.setter
    def visible(self, value):
        if self._visible != value:
            self._visible = value
            if self._group:
                self._group.hidden = not value

    def create_group(self):
        """Create the displayio group for this element."""
        raise NotImplementedError

    def update(self):
        """Update the display element if dirty."""
        raise NotImplementedError


class ValueBar(DisplayElement):
    """
    A bar display element showing a value (0-127) with label.

    Used for CC value display, expression pedals, etc.
    Optimized to only update the inner bar when value changes.
    """

    def __init__(self, x, y, width, height, label_text="", color="cyan", font=None):
        super().__init__(x, y, width, height)
        self._label_text = label_text
        self._color = color
        self._value = 0
        self._font = font

        # Display objects (created lazily)
        self._outer_rect = None
        self._inner_rect = None
        self._label = None

        # Track what changed
        self._value_dirty = False
        self._color_dirty = False
        self._label_dirty = False

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, val):
        val = max(0, min(127, int(val)))
        if self._value != val:
            self._value = val
            self._value_dirty = True
            self.mark_dirty()

    @property
    def color(self):
        return self._color

    @color.setter
    def color(self, val):
        if self._color != val:
            self._color = val
            self._color_dirty = True
            self.mark_dirty()

    @property
    def label_text(self):
        return self._label_text

    @label_text.setter
    def label_text(self, val):
        if self._label_text != val:
            self._label_text = val
            self._label_dirty = True
            self.mark_dirty()

    def create_group(self):
        """Create the displayio group for this bar."""
        self._group = displayio.Group()

        # Outer rectangle (background)
        bg_color = PALETTE.dark(self._color)
        self._outer_rect = Rect(
            self.x, self.y, self.width, self.height,
            fill=PALETTE.to_displayio(bg_color),
            outline=PALETTE.to_displayio('white'),
            stroke=1
        )
        self._group.append(self._outer_rect)

        # Inner rectangle (value bar)
        bar_width = self._calculate_bar_width()
        fg_color = PALETTE.get(self._color)
        self._inner_rect = Rect(
            self.x + 1, self.y + 1,
            max(1, bar_width), self.height - 2,
            fill=PALETTE.to_displayio(fg_color)
        )
        self._group.append(self._inner_rect)

        # Label
        if self._font:
            self._label = label.Label(
                self._font,
                text=self._label_text,
                color=PALETTE.to_displayio('white'),
                anchor_point=(0.5, 0.5),
                anchored_position=(self.width // 2, self.height // 2)
            )
            label_group = displayio.Group(x=self.x, y=self.y)
            label_group.append(self._label)
            self._group.append(label_group)

        self.clear_dirty()
        return self._group

    def _calculate_bar_width(self):
        """Calculate the width of the value bar."""
        return int((self._value / 127) * (self.width - 2))

    def update(self):
        """Update the bar if dirty. Returns True if updated."""
        if not self.dirty or not self._group:
            return False

        if self._color_dirty:
            # Update outer rect color
            bg_color = PALETTE.dark(self._color)
            # Note: In CircuitPython, we'd need to recreate the rect
            # or use a TileGrid with palette for efficient updates
            self._color_dirty = False

        if self._value_dirty:
            # Update inner rect width by recreating it
            # (More efficient approaches exist with TileGrid)
            bar_width = self._calculate_bar_width()
            fg_color = PALETTE.get(self._color)
            new_rect = Rect(
                self.x + 1, self.y + 1,
                max(1, bar_width), self.height - 2,
                fill=PALETTE.to_displayio(fg_color)
            )
            self._group[1] = new_rect
            self._inner_rect = new_rect
            self._value_dirty = False

        if self._label_dirty and self._label:
            self._label.text = self._label_text
            self._label_dirty = False

        self.clear_dirty()
        return True


class TextPanel(DisplayElement):
    """
    A text display panel for showing status messages, song names, etc.

    Supports multi-line text with automatic word wrapping.
    """

    def __init__(self, x, y, width, height, text="", color="white",
                 bg_color="black", font=None, max_chars_per_line=10):
        super().__init__(x, y, width, height)
        self._text = text
        self._color = color
        self._bg_color = bg_color
        self._font = font
        self._max_chars = max_chars_per_line

        self._bg_rect = None
        self._label = None
        self._text_dirty = False

    @property
    def text(self):
        return self._text

    @text.setter
    def text(self, val):
        if self._text != val:
            self._text = val
            self._text_dirty = True
            self.mark_dirty()

    def _wrap_text(self, text):
        """Wrap text to fit within max_chars per line."""
        if len(text) <= self._max_chars:
            return text

        words = text.split()
        lines = []
        current_line = ""

        for word in words:
            # Truncate long words
            if len(word) > self._max_chars:
                word = word[:self._max_chars]

            test_line = f"{current_line} {word}".strip() if current_line else word

            if len(test_line) <= self._max_chars:
                current_line = test_line
            else:
                if current_line:
                    lines.append(current_line)
                current_line = word

                # Limit to 2 lines
                if len(lines) >= 2:
                    break

        if current_line and len(lines) < 2:
            lines.append(current_line)

        return "\n".join(lines)

    def create_group(self):
        """Create the displayio group for this panel."""
        self._group = displayio.Group()

        # Background
        self._bg_rect = Rect(
            self.x, self.y, self.width, self.height,
            fill=PALETTE.to_displayio(PALETTE.dark(self._bg_color)),
            outline=PALETTE.to_displayio('white'),
            stroke=1
        )
        self._group.append(self._bg_rect)

        # Text label
        if self._font:
            wrapped = self._wrap_text(self._text)
            self._label = label.Label(
                self._font,
                text=wrapped,
                color=PALETTE.to_displayio(self._color),
                line_spacing=0.95,
                anchor_point=(0.5, 0.5),
                anchored_position=(self.width // 2, self.height // 2)
            )
            label_group = displayio.Group(x=self.x, y=self.y)
            label_group.append(self._label)
            self._group.append(label_group)

        self.clear_dirty()
        return self._group

    def update(self):
        """Update the panel if dirty."""
        if not self.dirty or not self._group:
            return False

        if self._text_dirty and self._label:
            self._label.text = self._wrap_text(self._text)
            self._text_dirty = False

        self.clear_dirty()
        return True


# ═══════════════════════════════════════════════════════════════════════════════
# DISPLAY MANAGER
# ═══════════════════════════════════════════════════════════════════════════════

class DisplayManager:
    """
    Main display manager for the ST7789 TFT.

    Handles:
    - Display initialization
    - Element management and layering
    - Efficient batch updates
    - Mode switching (normal/tuner)
    """

    # Display dimensions
    WIDTH = 240
    HEIGHT = 240

    # Font paths
    FONT_SMALL = "/fonts/PTSans-Regular-20.pcf"
    FONT_LARGE = "/fonts/PTSans-NarrowBold-54.pcf"
    FONT_XLARGE = "/fonts/PTSans-Bold-60.pcf"

    # NVM byte index used for hard-reset guard
    _NVM_RESET_FLAG = 0
    _NVM_RESET_MAGIC = 0xAA

    def __init__(self):
        self._display = None
        self._root_group = None
        self._backlight = None
        self._spi = None
        self._elements = {}
        self._layers = {}  # Named layers for mode switching

        # Fonts (loaded lazily)
        self._fonts = {}

        # Current mode
        self._mode = 'normal'

    def init(self, brightness=0.8):
        """Initialize the display hardware and backlight."""
        # Release any existing displays from previous run
        displayio.release_displays()
        gc.collect()

        # Clear NVM reset flag on entry (we made it to init)
        self._nvm_clear_flag()

        # Set up backlight PWM
        self._backlight = pwmio.PWMOut(pins.TFT_PWM, frequency=1000)
        self.set_brightness(brightness)

        # Set up SPI bus (may fail if pins are still claimed after soft reset)
        try:
            self._spi = busio.SPI(pins.SPI_CLK, MOSI=pins.SPI_MOSI)
        except ValueError:
            # SPI pins claimed despite release_displays() + gc.collect().
            # RP2040 soft resets don't fully release SPI (CP issue #4838).
            # Use NVM-guarded hard reset as last resort.
            self._nvm_guarded_reset()
            # If we get here, we already tried a reset — re-raise
            raise

        while not self._spi.try_lock():
            pass
        self._spi.configure(baudrate=24000000)
        self._spi.unlock()

        # Create display bus (fourwire module in CP10+)
        display_bus = fourwire.FourWire(
            self._spi,
            command=pins.TFT_DC,
            chip_select=pins.TFT_CS,
            reset=None,
            baudrate=24000000
        )

        # Create display
        self._display = ST7789(
            display_bus,
            width=self.WIDTH,
            height=self.HEIGHT,
            rowstart=80,
            rotation=180
        )

        # Create root group
        self._root_group = displayio.Group()
        self._display.root_group = self._root_group

    def _nvm_clear_flag(self):
        """Clear the NVM hard-reset flag."""
        try:
            import microcontroller
            if microcontroller.nvm[self._NVM_RESET_FLAG] == self._NVM_RESET_MAGIC:
                microcontroller.nvm[self._NVM_RESET_FLAG] = 0x00
        except Exception:
            pass

    def _nvm_guarded_reset(self):
        """Hard reset the MCU if we haven't already tried (prevents loops)."""
        import microcontroller
        if microcontroller.nvm[self._NVM_RESET_FLAG] != self._NVM_RESET_MAGIC:
            print("  SPI pins claimed, forcing hard reset...")
            microcontroller.nvm[self._NVM_RESET_FLAG] = self._NVM_RESET_MAGIC
            microcontroller.reset()
        # If flag was already set, a previous reset didn't help — don't loop
        print("  SPI pins still claimed after hard reset")

    def set_brightness(self, brightness):
        """Set backlight brightness (0.0 - 1.0)."""
        if self._backlight:
            duty = int(max(0.0, min(1.0, brightness)) * 65535)
            self._backlight.duty_cycle = duty

    def load_font(self, name):
        """Load a font (cached). 'builtin' returns terminalio.FONT."""
        if name not in self._fonts:
            if name == 'builtin':
                self._fonts[name] = terminalio.FONT
            else:
                path = getattr(self, f'FONT_{name.upper()}', None)
                if path:
                    try:
                        self._fonts[name] = bitmap_font.load_font(path)
                    except OSError:
                        print(f"  Font not found: {path}")
                        self._fonts[name] = None
                else:
                    self._fonts[name] = None
        return self._fonts[name]

    def create_layer(self, name):
        """Create a new display layer."""
        layer = displayio.Group()
        self._layers[name] = layer
        self._root_group.append(layer)
        return layer

    def get_layer(self, name):
        """Get an existing layer."""
        return self._layers.get(name)

    def show_layer(self, name):
        """Show a specific layer, hide others."""
        for layer_name, layer in self._layers.items():
            layer.hidden = (layer_name != name)

    def add_element(self, name, element, layer='default'):
        """Add an element to the display."""
        self._elements[name] = element

        # Ensure layer exists
        if layer not in self._layers:
            self.create_layer(layer)

        # Create and add the element's group
        group = element.create_group()
        self._layers[layer].append(group)

    def get_element(self, name):
        """Get an element by name."""
        return self._elements.get(name)

    def update(self):
        """
        Update all dirty elements.

        Returns number of elements updated.
        """
        updated = 0
        for element in self._elements.values():
            if element.dirty and element.visible:
                if element.update():
                    updated += 1
        return updated

    def set_mode(self, mode):
        """
        Switch display mode (e.g., 'normal', 'tuner').

        Args:
            mode: Mode name (must match a layer name)
        """
        if mode != self._mode:
            self._mode = mode
            self.show_layer(mode)

    def deinit(self):
        """Release display resources."""
        if self._backlight:
            self._backlight.deinit()
            self._backlight = None
        if self._display:
            displayio.release_displays()
            self._display = None
        if self._spi:
            self._spi.deinit()
            self._spi = None
