"""
MIDICaptain Remedy - CircuitPython Entry Point

Copy this file (along with the lib/ folder and config/ folder) to the
MIDICAPTAIN device to run the firmware.

Boot mode:
- Hold Switch1 (GP1) during power-on to enable USB drive for file access
- Normal boot (no switch held) runs the firmware
"""

# Import and run the main application
from main import main

main()
