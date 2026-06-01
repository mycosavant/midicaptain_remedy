#!/usr/bin/env python3
"""sysex_decode.py — decode Roland / BOSS Katana SysEx captured off the wire.

The decode-side companion to `midimon.ps1` (which shows the MIDI the device
emits as raw hex) and the third observation surface for validating
**device sync**: it turns the boot RQ1 sweep and the amp's DT1 replies into
human-readable lines — operation, the parameter the address names, the value,
and a checksum check.

It is a host-side mirror of `firmware/src/midi/katana.rs` (Roland ID, Katana
model id, DT1/RQ1 ops, the parameter address map, the 7-bit checksum, 11-bit
encoding). Keep the two in lockstep: if you add an address there, add it to
`PARAMS` here.

No dependencies — pure Python 3.

Usage
-----
    # Decode bytes given on the command line (with or without 0x / commas):
    python sysex_decode.py F0 41 00 00 00 00 33 12 00 00 04 20 02 28 F7

    # Pipe a hex capture in (e.g. from midimon's min-hex format):
    ./midimon.ps1 -Format min-hex | python sysex_decode.py

    # Decode a saved capture file:
    python sysex_decode.py --file capture.hex

    # Verify the decoder itself against known-good vectors:
    python sysex_decode.py --selftest

Input is tolerant: any run of two-hex-digit tokens is read as bytes, so you can
paste lines straight from a monitor. Bytes are split into SysEx frames on
`F0` (start) / `F7` (end); anything between frames is ignored.
"""

from __future__ import annotations

import argparse
import re
import sys

# ── Protocol constants (mirror firmware/src/midi/katana.rs) ─────────────────
SYSEX_START = 0xF0
SYSEX_END = 0xF7
ROLAND_ID = 0x41
KATANA_MODEL_ID = (0x00, 0x00, 0x00, 0x33)
OP_DT1 = 0x12  # Data Set 1 — write / report
OP_RQ1 = 0x11  # Data Request 1 — read

AMP_TYPES = {0: "Acoustic", 1: "Clean", 2: "Crunch", 3: "Lead", 4: "Brown"}
PRESETS = {0: "Panel", 1: "CH1", 2: "CH2", 3: "CH3", 4: "CH4"}


def roland_checksum(data: bytes) -> int:
    """Roland 7-bit checksum over `data` (port of `roland_checksum`)."""
    accum = sum(data) & 0x7F
    return (128 - accum) & 0x7F


def decode_11bit(hi: int, lo: int) -> int:
    """Decode Roland's 11-bit [high, low] pair (port of `decode_11bit`)."""
    return ((hi & 0x0F) << 7) | (lo & 0x7F)


# ── Parameter address map (address tuple → (name, value-formatter)) ─────────
# The formatter takes the DT1 data bytes and returns a human string. For RQ1
# the data is the 4-byte length field, handled separately.

def _amp_type(d: bytes) -> str:
    v = d[0] if d else None
    return f"{v} ({AMP_TYPES.get(v, '?')})" if v is not None else "?"


def _preset(d: bytes) -> str:
    # recall_preset emits [0x00, preset]; tolerate a 1-byte form too.
    v = d[1] if len(d) >= 2 else (d[0] if d else None)
    return f"{v} ({PRESETS.get(v, '?')})" if v is not None else "?"


def _scalar(d: bytes) -> str:
    return str(d[0]) if d else "?"


def _bool(d: bytes) -> str:
    if not d:
        return "?"
    return "ON" if d[0] else "OFF"


def _delay_time(d: bytes) -> str:
    return f"{decode_11bit(d[0], d[1])} ms" if len(d) >= 2 else "?"


def _editor(d: bytes) -> str:
    if not d:
        return "?"
    return "ENTER" if d[0] else "EXIT"


PARAMS: dict[tuple, tuple] = {
    (0x00, 0x01, 0x00, 0x00): ("RecallPreset", _preset),
    (0x00, 0x00, 0x04, 0x20): ("AmpType", _amp_type),
    (0x00, 0x00, 0x04, 0x21): ("Gain", _scalar),
    (0x00, 0x00, 0x04, 0x22): ("Volume", _scalar),
    (0x60, 0x00, 0x01, 0x5D): ("WahPosition", _scalar),
    (0x60, 0x00, 0x00, 0x30): ("Boost", _bool),
    (0x60, 0x00, 0x01, 0x40): ("Mod", _bool),
    (0x60, 0x00, 0x05, 0x60): ("Delay", _bool),
    (0x60, 0x00, 0x06, 0x10): ("Reverb", _bool),
    (0x60, 0x00, 0x05, 0x62): ("DelayTime", _delay_time),
    (0x7F, 0x00, 0x00, 0x01): ("EditorMode", _editor),
}


def _hex(bs) -> str:
    return " ".join(f"{b:02X}" for b in bs)


def decode_frame(frame: list[int]) -> str:
    """Decode one complete `F0 .. F7` frame into a human line."""
    if len(frame) < 2 or frame[0] != SYSEX_START or frame[-1] != SYSEX_END:
        return f"malformed frame: {_hex(frame)}"
    body = frame[1:-1]  # between the delimiters
    if not body:
        return "empty SysEx"
    if body[0] != ROLAND_ID:
        return f"non-Roland SysEx (mfr 0x{body[0]:02X}, {len(frame)} bytes): {_hex(frame)}"

    # F0 41 <dev> <model[4]> <op> <addr[4]> <data..> <cksum> F7
    if len(body) < 11:  # 41 dev m0..m3 op a0..a3 (no data, no cksum yet)
        return f"truncated Roland SysEx: {_hex(frame)}"
    dev = body[1]
    model = tuple(body[2:6])
    op = body[6]
    addr = tuple(body[7:11])
    rest = body[11:]  # data.. + checksum
    if not rest:
        return f"Roland SysEx missing checksum: {_hex(frame)}"
    data = bytes(rest[:-1])
    checksum = rest[-1]
    expect = roland_checksum(bytes(addr) + data)
    cksum_note = "" if checksum == expect else f"  !! CHECKSUM 0x{checksum:02X}, expected 0x{expect:02X}"

    model_note = "" if model == KATANA_MODEL_ID else f" model={_hex(model)}"
    dev_note = "" if dev == 0x00 else f" dev=0x{dev:02X}"

    op_name = {OP_DT1: "DT1", OP_RQ1: "RQ1"}.get(op, f"op=0x{op:02X}")
    name, fmt = PARAMS.get(addr, (None, None))
    addr_label = name if name else f"addr {_hex(addr)}"

    if op == OP_RQ1:
        # data is the 4-byte length field [0,0,0,N] (low byte the count).
        length = data[-1] if data else "?"
        return f"RQ1 read  {addr_label}  len={length}{model_note}{dev_note}{cksum_note}"
    if op == OP_DT1:
        value = fmt(data) if fmt else _hex(data)
        return f"DT1 set   {addr_label} = {value}  [{_hex(data)}]{model_note}{dev_note}{cksum_note}"
    return f"{op_name}  {addr_label}  data=[{_hex(data)}]{model_note}{dev_note}{cksum_note}"


def frames_from_bytes(stream: list[int]):
    """Yield complete `F0 .. F7` frames from a flat byte stream, skipping
    anything outside a frame (and dropping an unterminated trailing frame)."""
    cur: list[int] | None = None
    for b in stream:
        if b == SYSEX_START:
            cur = [b]
        elif cur is not None:
            cur.append(b)
            if b == SYSEX_END:
                yield cur
                cur = None


_HEX_TOKEN = re.compile(r"(?:0x)?([0-9a-fA-F]{2})\b")


def bytes_from_text(text: str) -> list[int]:
    """Extract a byte stream from arbitrary text: every two-hex-digit token."""
    return [int(m.group(1), 16) for m in _HEX_TOKEN.finditer(text)]


def decode_text(text: str) -> list[str]:
    stream = bytes_from_text(text)
    return [decode_frame(f) for f in frames_from_bytes(stream)]


# ── Self-test: decode known-good vectors (mirror midi_engine_test.rs) ───────
def selftest() -> int:
    cases = [
        # (hex, expected substring)
        ("F0 41 00 00 00 00 33 12 00 00 04 21 32 29 F7", "DT1 set   Gain = 50"),
        ("F0 41 00 00 00 00 33 12 00 00 04 20 02 5A F7", "DT1 set   AmpType = 2 (Crunch)"),
        ("F0 41 00 00 00 00 33 12 00 01 00 00 00 03 7C F7", "DT1 set   RecallPreset = 3 (CH3)"),
        ("F0 41 00 00 00 00 33 11 00 00 04 21 00 00 00 01 5A F7", "RQ1 read  Gain  len=1"),
        ("F0 41 00 00 00 00 33 11 00 00 04 20 00 00 00 01 5B F7", "RQ1 read  AmpType  len=1"),
        ("F0 41 00 00 00 00 33 12 7F 00 00 01 01 7F F7", "DT1 set   EditorMode = ENTER"),
        ("F0 41 00 00 00 00 33 12 60 00 05 62 03 74 42 F7", "DT1 set   DelayTime = 500 ms"),
        # corrupt checksum is flagged, not silently accepted
        ("F0 41 00 00 00 00 33 12 00 00 04 21 32 00 F7", "CHECKSUM"),
        # foreign manufacturer
        ("F0 43 12 00 F7", "non-Roland SysEx (mfr 0x43"),
    ]
    ok = 0
    for hexs, expect in cases:
        out = decode_text(hexs)
        line = out[0] if out else "<no frame>"
        passed = expect in line
        ok += passed
        print(f"  [{'PASS' if passed else 'FAIL'}] {line!r}" + ("" if passed else f"  (want {expect!r})"))
    total = len(cases)
    print(f"sysex_decode self-test: {ok}/{total} " + ("ALL PASS" if ok == total else "FAILED"))
    return 0 if ok == total else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="Decode Roland/Katana SysEx off the wire.")
    ap.add_argument("bytes", nargs="*", help="hex bytes to decode (e.g. F0 41 ... F7)")
    ap.add_argument("--file", help="read hex from a file instead of args/stdin")
    ap.add_argument("--selftest", action="store_true", help="decode known-good vectors and exit")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    if args.file:
        with open(args.file, "r", encoding="utf-8", errors="replace") as fh:
            text = fh.read()
    elif args.bytes:
        text = " ".join(args.bytes)
    elif not sys.stdin.isatty():
        # Piped input: decode line-by-line so a live monitor stream prints as
        # it arrives, while still reassembling frames that span lines.
        buf: list[int] = []
        any_out = False
        for raw in sys.stdin:
            buf.extend(bytes_from_text(raw))
            # Drain whole frames as they complete; keep a partial tail.
            while SYSEX_END in buf:
                end = buf.index(SYSEX_END)
                # find the frame start at/<= end
                try:
                    start = max(i for i in range(end + 1) if buf[i] == SYSEX_START)
                except ValueError:
                    del buf[: end + 1]
                    continue
                print(decode_frame(buf[start : end + 1]))
                any_out = True
                del buf[: end + 1]
        if not any_out:
            print("no SysEx frames found in input", file=sys.stderr)
            return 1
        return 0
    else:
        ap.print_help()
        return 2

    lines = decode_text(text)
    if not lines:
        print("no SysEx frames found in input", file=sys.stderr)
        return 1
    for line in lines:
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
