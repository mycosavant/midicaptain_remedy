# MIDICaptain Remedy - boot.py
#
# IMPORTANT: In CircuitPython 10 on RP2040, `import storage` in boot.py
# claims SPI1 pins (GP14/GP15) as a side effect, breaking display init.
# Until this CP10 bug is resolved, boot.py must NOT import storage.
#
# Consequence: USB drive is always enabled (CP10 default).
# This is fine for development. For production deployment,
# investigate CP10 storage module fix or use boot_out.txt workaround.
pass
