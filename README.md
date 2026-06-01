# Introduction

This is an effort to reverse-engineer and develop alternative firmware for the [Paint Audio MIDI Captain](https://paintaudio.com/products/paint-audio-midi-captain-blue).

It is based on reverse engineering and is made available to the public in the hope that it will be useful, but
I offer

**no warranty whatsoever on anything (if you break it, YOU ARE ON YOUR OWN!!!!)**.

This information should also apply to other Paint Audio products:
- [Midi Captain STD Version Black](https://paintaudio.com/products/midi-captain-std-vesion-black) (Same with no wireless module)
- [Midi Captain Mini 6](https://paintaudio.com/products/midi-captain-mini-6-controller-with-hid-multi-state-cycling)
- [Midi Captain Nano 4](https://paintaudio.com/products/midi-captain-nano-4-controller-with-hid-multi-state-cycling)
- [Midi Captain Duo](https://paintaudio.com/products/midi-captain-duo-controller-with-hid-multi-state-cycling-1)
- [Midi Captain One](https://paintaudio.com/products/midi-captain-one-controller-with-hid-multi-state-cycling-1)

which appear to be based on the same microcontroller (RP2040) and CircuitPython.
However, as I do not own them, I cannot verify the information.

> **Project status — custom firmware:** Beyond documenting the OEM hardware, this
> repository now hosts two custom firmware efforts. [`remedy/`](./remedy/) was the
> first attempt: a configuration-driven CircuitPython firmware (bumped to
> CircuitPython 10). Having run into the limitations of CircuitPython for this
> use case, the work moved to a lower-level language: the current focus is the
> Rust + [Embassy](https://embassy.dev) firmware in [`firmware/`](./firmware/).
> Its [`README.md`](./firmware/README.md) and [`HARDWARE.md`](./firmware/HARDWARE.md)
> capture the latest reverse-engineering findings (verified pin map, SWD debug
> pads, display orientation, and more).

# General Description

The board is built around a Raspberry Pi RP2040 microcontroller ([RP2040 Datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf)) and is programmed using [CircuitPython](https://circuitpython.org).

The CircuitPython version on the board, as of firmware 4.0, is 7.3.1 (see boot_out.txt).

Links:
- [RP2040 Datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf)
- [CircuitPython](https://circuitpython.org)
- [Paintaudio Recover uf2 file](https://cdn.shopify.com/s/files/1/0656/8312/8548/files/midicpPico.uf2)
- [CircuitPython 7.x Library Bundle](https://github.com/adafruit/Adafruit_CircuitPython_Bundle/releases/download/20231003/adafruit-circuitpython-bundle-7.x-mpy-20231003.zip)
- [Paintaudio Midi Captain 3in1 Firmware 4.0](https://cdn.shopify.com/s/files/1/0656/8312/8548/files/FW-MIDICP-10-switch-3in1-V4.0.zip?v=1712483089)

## Going into update mode

As described on the main product page [here](https://paintaudio.com/products/paint-audio-midi-captain-blue), to modify the CircuitPython code you must enter Update Mode.

On the MIDI Captain this is done by pressing switch0 (the top-left switch) while powering up with the device connected to a computer via USB.
This exposes the MIDICAPTAIN drive, where you can load new firmware and modify the code.

## Going into CircuitPython bootloader mode

On the board there is a 2-pin header; if you short it and reboot, the device enters CircuitPython recovery mode.

**Don't mess with it if you don't know what you are doing.**

- [Paintaudio Recover uf2 file](https://cdn.shopify.com/s/files/1/0656/8312/8548/files/midicpPico.uf2)

## CircuitPython Serial Debugging Console

1. Install the [mu editor][https://learn.adafruit.com/welcome-to-circuitpython/installing-mu-editor]
2. Start the MIDI Captain in firmware update mode (switch 0 pressed at boot)
3. Using the mu editor, open the boot.py file and the serial console
4. Comment out the following lines to enable autoreload
   ```python
   # import supervisor
   # supervisor.disable_autoreload()
   ```
5. Restart in firmware update mode. You will see:
   ```shell
   Adafruit CircuitPython 7.3.1 on 2022-06-22; Raspberry Pi Pico with rp2040
   >>>
   ```
6. If you modify the boot.py or code.py files you will see
   ```shell
   Auto-reload is on. Simply save files over USB to run them or enter REPL to disable.
   code.py output:
   ```
7. You can stop the boot by pressing `Enter` at the prompt, or soft-reboot with `Ctrl+D`.


### Relevant Files and Folders

Based on the [normal+geek_super 3in1 FW4.0](https://cdn.shopify.com/s/files/1/0656/8312/8548/files/FW-MIDICP-10-switch-3in1-V4.0.zip).

```shell
.
├── boot.py                                # Raspbe
├── boot_out.txt
├── code.py                                # Main Python Code
├── lib
│   ├── adafruit_bitmap_font               # Display Libraries
│   ├── adafruit_display_notification
│   ├── adafruit_display_shapes
│   ├── adafruit_display_text
│   ├── adafruit_displayio_layout
│   ├── adafruit_imageload
│   ├── adafruit_progressbar
│   ├── adafruit_st7789.mpy
│   ├── adafruit_framebuf.mpy

│   ├── adafruit_midi                     # Midi Library
│   ├── adafruit_bus_device               #
│   ├── adafruit_hid                      # USB Keyboard
│   ├── adafruit_io                       #
│   ├── adafruit_minimqtt                 # MQTT library?
│   ├── asyncio/                          #
│   ├── adafruit_ticks.mpy                #
│   ├── neopixel.mpy
│   ├── midicaptain.mpy                   # MIDI Captain 
│   ├── midicaptain10s.mpy                # MIDI Captain 
│   ├── midicaptain_ledon.mpy             # MIDI Captain 
│   ├── midigeek.mpy                      # MIDI Captain Geek Mode
│   └── midigeek_C.mpy                    # MIDI Captain 
```

#### boot_out.txt

```
Adafruit CircuitPython 7.3.1 on 2022-06-22; Raspberry Pi Pico with rp2040
Board ID:raspberry_pi_pico
```

#### boot.py

```python
import storage
import board
import digitalio
import time

# remove this two lines to auto reload the code when is modified
import supervisor
supervisor.disable_autoreload()

switch = digitalio.DigitalInOut(board.GP1)
switch.direction = digitalio.Direction.INPUT
switch.pull = digitalio.Pull.UP
time.sleep(0.05)

if switch.value is True:
    storage.disable_usb_drive()
    storage.remount("/", readonly=False)
else:
    storage.remount("/", readonly=False)
    m = storage.getmount("/")
    m.label = "MIDICAPTAIN"
    storage.enable_usb_drive()
    storage.remount("/", readonly=True)
```

#### code.py

A large block of code that selects the firmware mode.

# Peripherals

## MIDI Captain Blue

The board is built around a Raspberry Pi RP2040 microcontroller ([Datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf)),
with the following peripherals:

- 10 footswitches
- 30 individually addressable RGB LEDs
- MIDI In/Out
- 2 expression-pedal inputs
- Battery
- Wireless module

### GPIO Assignment

| GPIO | PERIPHERAL       | board lib pin / notes?  |
| ---- | ---------------- | ----------------------- |
| GP0  | encoder switch   |                         |
| GP1  | switch1          |                         |
| GP2  | encoder A        |                         |
| GP3  | encoder B        |                         |
| GP4  |                  |                         |
| GP5  |                  |                         |
| GP6  |                  |                         |
| GP7  | neopixel GPIO    |                         |
| GP8  | tft_pwm          |                         |
| GP9  | switchA          |                         |
| GP10 | switchB          |                         |
| GP11 | switchC          |                         |
| GP12 | tft_dc           | (debug_port??)          |
| GP13 | tft_cs           | (debug_port??)          |
| GP14 | tft_spi_clk      |                         |
| GP15 | tft_spi_mosi     |                         |
| GP16 | uart_midi_tx     |                         |
| GP17 | uart_midi_rx     |                         |
| GP18 | switchD          |                         |
| GP19 | switchDown       |                         |
| GP20 | switchUp         |                         |
| GP21 |                  |                         |
| GP22 |                  |                         |
| GP23 | switch4          | SMPS_MODE               |
| GP24 | switch3          | VBUS_SENSE              |
| GP25 | switch2          | LED                     |
| GP26 |                  | GP26_A0                 |
| GP27 | ExpPdl1          | GP27_A1                 |
| GP28 | ExpPdl2          | GP28_A2                 |
| GP29 |                  | VOLTAGE_MONITOR         |

# Reverse engineering code

## MIDI UART

### UART Pins

UART pins can be assigned to any GPIO.

### GPIO Loopback Test

1. Connect a MIDI cable between MIDI In and Out
2. Run the following

## NeoPixel LEDs

30 RGB LEDs on the board.

### Test Code

[scripts/led.py](./scripts/led.py)

## Display

Adafruit 1.54" 240x240 Wide Angle TFT LCD Display with MicroSD | ST7789 with EYESPI Connector

- https://www.adafruit.com/product/3787

  1.54" 240x240 Color IPS TFT Display

- https://www.adafruit.com/product/4421

## Pin Assignments

```python
tft_pwm = board.GP8
tft_dc = board.GP12
tft_cs = board.GP13
spi_clk = board.GP14
spi_mosi = board.GP15

spi = busio.SPI(spi_clk, spi_mosi)

display_bus = displayio.FourWire(spi, command=tft_dc, chip_select=tft_cs)
display = ST7789(
    display_bus, width=240, height=240,
    rowstart=80,
    # colstart=53,
    rotation=180,
)
```

## Wireless Module

The wireless module is a "wireless mouse"-style serial RF IC.

[BK2461 Datasheet](https://www.alldatasheet.com/datasheet-pdf/pdf/1132247/ETC2/BK2461.html)

![Image](./docs/images/midicaptain-rf.jpg?raw=true "rf module BK2462")
