#!/usr/bin/env python3
"""
Host client for the MIDI Captain config-sync CDC link.

Mirrors the device codec (`firmware/src/proto.rs`): COBS framing + CRC-16/
CCITT-FALSE. Phase B-2a sends a HELLO frame and verifies the reply, proving the
USB-CDC link + framing end-to-end. GET_CONFIG / SET_CONFIG land with B-2b.

Run from **Windows** (the board enumerates as a USB-CDC serial port; WSL can't
see USB COM ports):

    pip install pyserial
    python firmware/scripts/cdc_config_client.py COM5

Find the port with: python -m serial.tools.list_ports
"""
import sys

try:
    import serial  # pyserial
except ImportError:
    sys.exit("This needs pyserial:  pip install pyserial")

PROTO_VERSION = 1
CMD_HELLO, CMD_GET_CONFIG, CMD_SET_CONFIG, CMD_REBOOT, CMD_ERROR = (
    0x01, 0x02, 0x03, 0x04, 0xFF,
)


def crc16(data: bytes) -> int:
    """CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF)."""
    crc = 0xFFFF
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def cobs_encode(data: bytes) -> bytes:
    out = bytearray([0])
    code_pos, code = 0, 1
    for b in data:
        if b == 0:
            out[code_pos] = code
            code_pos, code = len(out), 1
            out.append(0)
        else:
            out.append(b)
            code += 1
            if code == 0xFF:
                out[code_pos] = code
                code_pos, code = len(out), 1
                out.append(0)
    out[code_pos] = code
    return bytes(out)


def cobs_decode(data: bytes) -> bytes:
    out = bytearray()
    i = 0
    while i < len(data):
        code = data[i]
        i += 1
        if code == 0:
            raise ValueError("bad COBS (interior zero)")
        for _ in range(1, code):
            out.append(data[i])
            i += 1
        if code != 0xFF and i < len(data):
            out.append(0)
    return bytes(out)


def encode_frame(cmd: int, seq: int, payload: bytes = b"") -> bytes:
    body = bytes([cmd, seq]) + payload
    body += crc16(body).to_bytes(2, "big")
    return cobs_encode(body) + b"\x00"


def decode_frame(frame: bytes):
    body = cobs_decode(frame)
    if len(body) < 4:
        raise ValueError("short frame")
    if crc16(body[:-2]) != int.from_bytes(body[-2:], "big"):
        raise ValueError("CRC mismatch")
    return body[0], body[1], body[2:-2]


def read_frame(ser) -> bytes:
    """Read bytes until the 0x00 frame delimiter."""
    buf = bytearray()
    while True:
        b = ser.read(1)
        if not b:
            raise TimeoutError("no reply — device flashed? correct COM port?")
        if b == b"\x00":
            return bytes(buf)
        buf += b


def hello(ser) -> int:
    ser.reset_input_buffer()
    ser.write(encode_frame(CMD_HELLO, 0x42, bytes([PROTO_VERSION])))
    cmd, seq, payload = decode_frame(read_frame(ser))
    assert cmd == CMD_HELLO, f"expected HELLO reply, got cmd={cmd:#04x}"
    assert seq == 0x42, f"seq not echoed (got {seq:#04x})"
    assert payload and payload[0] >= 1, f"bad proto version payload {list(payload)}"
    return payload[0]


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(f"usage: {sys.argv[0]} <PORT>   (e.g. COM5)")
    with serial.Serial(sys.argv[1], 115200, timeout=2) as ser:
        version = hello(ser)
        print(f"OK — device HELLO, protocol v{version}, seq echoed.")


if __name__ == "__main__":
    main()
