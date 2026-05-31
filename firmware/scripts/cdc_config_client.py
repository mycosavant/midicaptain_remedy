#!/usr/bin/env python3
"""
Host client for the MIDI Captain config-sync CDC link.

Mirrors the device codec (`firmware/src/proto.rs`): COBS framing + CRC-16/
CCITT-FALSE, and the on-the-wire config model (`firmware/src/config/mod.rs`):
a compact `postcard` blob. Subcommands:

    hello       probe the link (handshake; prints the protocol version)
    get         read the device's live config, decode + pretty-print it
                (optionally save the JSON and/or the raw blob)
    set FILE    push a config from FILE (.json = encode, .bin = raw blob)
    tweak       read → rename page 1 + recolor its first button → push it back
                (a one-shot visible change to confirm hot-reload on hardware)
    roundtrip   read → push the identical bytes → read again, assert unchanged
                (validates GET/SET/persist/reload without authoring anything)

Run from **Windows** (the board enumerates as a USB-CDC serial port; WSL can't
see USB COM ports):

    pip install pyserial
    python firmware/scripts/cdc_config_client.py COM5 get
    python firmware/scripts/cdc_config_client.py COM5 tweak

Find the port with: python -m serial.tools.list_ports
"""
import argparse
import json
import sys

try:
    import serial  # pyserial
except ImportError:
    sys.exit("This needs pyserial:  pip install pyserial")

# Windows consoles default to cp1252; config page names / labels (and our own
# output) may carry characters it can't encode. Force UTF-8 so printing a config
# never crashes the tool.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

PROTO_VERSION = 5  # v5: Action::Hid (keyboard/consumer); v4: cycles pool + Action::Cycle; v3 group; v2 midi_thru
CMD_HELLO, CMD_GET_CONFIG, CMD_SET_CONFIG, CMD_REBOOT, CMD_ERROR = (
    0x01, 0x02, 0x03, 0x04, 0xFF,
)
ERR_NAMES = {1: "BadCommand", 2: "BadPayload", 3: "StoreFailed"}

PAGE_BUTTONS = 10  # config::PAGE_BUTTONS — every page has exactly this many
# config::ThruRoutes fields, in serialized (declaration) order.
THRU_FIELDS = ("usb_to_din", "din_to_usb", "din_to_din", "usb_to_usb")
# config::CycleLong variant order (serde keys by position).
CYCLE_LONG = ("none", "reset", "reverse")


# ── wire codec (mirror of src/proto.rs) ─────────────────────────────────────
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


# ── postcard codec (mirror of src/config/mod.rs RuntimeConfig) ───────────────
# postcard encodes: seq (Vec) = varint(len) + elements; array [T; N] = N
# elements, no length prefix; str/String = varint(len) + utf-8; struct = fields
# in order; enum = varint(variant_index) + variant fields; u8 = one byte. All
# our lengths/discriminants are < 128, so every varint here is a single byte —
# but we encode/decode true LEB128 so the codec stays correct if the model grows.
_SYSEX = ["recall_preset", "amp_type", "gain", "volume"]  # SysexCmd variant order


def _varint_enc(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | 0x80 if n else b)
        if not n:
            return bytes(out)


class _Reader:
    def __init__(self, data: bytes):
        self.d, self.i = data, 0

    def u8(self) -> int:
        v = self.d[self.i]
        self.i += 1
        return v

    def varint(self) -> int:
        result, shift = 0, 0
        while True:
            b = self.u8()
            result |= (b & 0x7F) << shift
            if not (b & 0x80):
                return result
            shift += 7

    def s(self) -> str:
        n = self.varint()
        v = self.d[self.i:self.i + n].decode("utf-8")
        self.i += n
        return v


def _dec_action(r: _Reader) -> dict:
    disc = r.varint()
    if disc == 0:
        return {"type": "none"}
    if disc == 1:  # MidiCc { cc, value: CcValue }
        cc = r.u8()
        vdisc = r.varint()
        if vdisc == 0:      # Fixed(u8)
            value = r.u8()
        elif vdisc == 1:    # Toggle
            value = "toggle"
        elif vdisc == 2:    # Momentary
            value = "momentary"
        else:
            raise ValueError(f"unknown CcValue discriminant {vdisc}")
        return {"type": "cc", "cc": cc, "value": value}
    if disc == 2:  # ProgramChange { program }
        return {"type": "pc", "program": r.u8()}
    if disc == 3:  # Sysex(SysexCmd)
        cmd = _SYSEX[r.varint()]
        return {"type": "sysex", "cmd": cmd, "arg": r.u8()}
    if disc == 4:  # PageChange(u8)
        return {"type": "page_change", "page": r.u8()}
    if disc == 5:
        return {"type": "page_next"}
    if disc == 6:
        return {"type": "page_prev"}
    if disc == 7:
        return {"type": "tuner"}
    if disc == 8:  # ProgramChangeStep(i8) — postcard encodes i8 as one raw byte
        b = r.u8()
        return {"type": "pc_step", "step": b - 256 if b >= 128 else b}
    if disc == 9:  # Cycle(u8) — index into the config's cycle pool
        return {"type": "cycle", "index": r.u8()}
    if disc == 10:  # Hid(HidReport) — keyboard / consumer-control report
        hdisc = r.varint()
        if hdisc == 0:  # Key { keycode: u8, modifiers: u8 }
            return {"type": "hid", "hid": "key", "keycode": r.u8(), "modifiers": r.u8()}
        if hdisc == 1:  # Consumer { usage: u16 } — postcard varint
            return {"type": "hid", "hid": "consumer", "usage": r.varint()}
        raise ValueError(f"unknown HidReport discriminant {hdisc}")
    raise ValueError(f"unknown Action discriminant {disc}")


def _enc_action(a: dict) -> bytes:
    t = a["type"]
    if t == "none":
        return _varint_enc(0)
    if t == "cc":
        out = _varint_enc(1) + bytes([a["cc"]])
        v = a["value"]
        if v == "toggle":
            return out + _varint_enc(1)
        if v == "momentary":
            return out + _varint_enc(2)
        return out + _varint_enc(0) + bytes([int(v)])  # Fixed(u8)
    if t == "pc":
        return _varint_enc(2) + bytes([a["program"]])
    if t == "sysex":
        return _varint_enc(3) + _varint_enc(_SYSEX.index(a["cmd"])) + bytes([a["arg"]])
    if t == "page_change":
        return _varint_enc(4) + bytes([a["page"]])
    if t == "page_next":
        return _varint_enc(5)
    if t == "page_prev":
        return _varint_enc(6)
    if t == "tuner":
        return _varint_enc(7)
    if t == "pc_step":
        return _varint_enc(8) + bytes([a["step"] & 0xFF])  # i8 as one raw byte
    if t == "cycle":
        return _varint_enc(9) + bytes([a["index"]])
    if t == "hid":  # Hid(HidReport)
        h = a["hid"]
        if h == "key":  # Key { keycode: u8, modifiers: u8 }
            return _varint_enc(10) + _varint_enc(0) + bytes([a["keycode"], a["modifiers"]])
        if h == "consumer":  # Consumer { usage: u16 } — postcard varint
            return _varint_enc(10) + _varint_enc(1) + _varint_enc(a["usage"])
        raise ValueError(f"unknown hid report kind {h!r}")
    raise ValueError(f"unknown action type {t!r}")


# config::StepAction — the flat action subset a cycle step can hold.
def _dec_step(r: _Reader) -> dict:
    disc = r.varint()
    if disc == 0:  # MidiCc { cc, value } (a fixed u8 value, not CcValue)
        return {"type": "cc", "cc": r.u8(), "value": r.u8()}
    if disc == 1:  # ProgramChange { program }
        return {"type": "pc", "program": r.u8()}
    if disc == 2:  # Sysex(SysexCmd)
        return {"type": "sysex", "cmd": _SYSEX[r.varint()], "arg": r.u8()}
    raise ValueError(f"unknown StepAction discriminant {disc}")


def _enc_step(s: dict) -> bytes:
    t = s["type"]
    if t == "cc":
        return _varint_enc(0) + bytes([s["cc"], int(s["value"])])
    if t == "pc":
        return _varint_enc(1) + bytes([s["program"]])
    if t == "sysex":
        return _varint_enc(2) + _varint_enc(_SYSEX.index(s["cmd"])) + bytes([s["arg"]])
    raise ValueError(f"unknown step type {t!r}")


def decode_config(blob: bytes) -> dict:
    r = _Reader(blob)
    pages = []
    for _ in range(r.varint()):
        name = r.s()
        buttons = []
        for _ in range(PAGE_BUTTONS):
            label = r.s()
            color = [r.u8(), r.u8(), r.u8()]
            on_press = _dec_action(r)
            on_long_press = _dec_action(r)
            group = r.u8()  # mutual-exclusion radio group (0 = ungrouped)
            buttons.append({
                "label": label,
                "color": color,
                "on_press": on_press,
                "on_long_press": on_long_press,
                "group": group,
            })
        pages.append({"name": name, "buttons": buttons})
    # ThruRoutes: 4 bools, appended after pages (RuntimeConfig field order).
    midi_thru = {field: bool(r.u8()) for field in THRU_FIELDS}
    # Cycle pool: Vec<CycleDef>, appended after midi_thru.
    cycles = []
    for _ in range(r.varint()):
        steps = [_dec_step(r) for _ in range(r.varint())]
        long = CYCLE_LONG[r.varint()]
        cycles.append({"steps": steps, "long": long})
    if r.i != len(blob):
        raise ValueError(f"trailing bytes ({len(blob) - r.i}) after decode")
    return {"pages": pages, "midi_thru": midi_thru, "cycles": cycles}


def encode_config(cfg: dict) -> bytes:
    pages = cfg["pages"]
    if not pages:
        raise ValueError("config must have at least one page (device rejects empty)")
    out = bytearray(_varint_enc(len(pages)))
    for p in pages:
        nm = p["name"].encode("utf-8")
        out += _varint_enc(len(nm)) + nm
        btns = p["buttons"]
        if len(btns) != PAGE_BUTTONS:
            raise ValueError(f"each page needs exactly {PAGE_BUTTONS} buttons "
                             f"(page {p['name']!r} has {len(btns)})")
        for b in btns:
            lb = b["label"].encode("utf-8")
            out += _varint_enc(len(lb)) + lb
            out += bytes(b["color"])
            out += _enc_action(b["on_press"])
            out += _enc_action(b["on_long_press"])
            out.append(int(b.get("group", 0)) & 0xFF)  # radio group (0 = ungrouped)
    # ThruRoutes: 4 bools after pages. Tolerate older configs missing the field.
    thru = cfg.get("midi_thru", {})
    for field in THRU_FIELDS:
        out.append(1 if thru.get(field) else 0)
    # Cycle pool after midi_thru. Tolerate configs without a "cycles" key.
    cycles = cfg.get("cycles", [])
    out += _varint_enc(len(cycles))
    for c in cycles:
        steps = c["steps"]
        out += _varint_enc(len(steps))
        for s in steps:
            out += _enc_step(s)
        out += _varint_enc(CYCLE_LONG.index(c.get("long", "none")))
    return bytes(out)


# ── request/response ─────────────────────────────────────────────────────────
_seq = 0


def _next_seq() -> int:
    global _seq
    _seq = (_seq + 1) & 0xFF
    return _seq


def _txn(ser, cmd: int, payload: bytes = b""):
    """Send one frame and return the (cmd, payload) of the reply, raising on ERROR."""
    seq = _next_seq()
    ser.reset_input_buffer()
    ser.write(encode_frame(cmd, seq, payload))
    rcmd, rseq, rpayload = decode_frame(read_frame(ser))
    if rseq != seq:
        raise RuntimeError(f"seq mismatch (sent {seq:#04x}, got {rseq:#04x})")
    if rcmd == CMD_ERROR:
        code = rpayload[0] if rpayload else 0
        raise RuntimeError(f"device returned ERROR({ERR_NAMES.get(code, code)})")
    return rcmd, rpayload


def hello(ser) -> int:
    cmd, payload = _txn(ser, CMD_HELLO, bytes([PROTO_VERSION]))
    assert cmd == CMD_HELLO, f"expected HELLO reply, got cmd={cmd:#04x}"
    assert payload and payload[0] >= 1, f"bad proto version payload {list(payload)}"
    return payload[0]


def get_config(ser) -> bytes:
    cmd, payload = _txn(ser, CMD_GET_CONFIG)
    assert cmd == CMD_GET_CONFIG, f"expected GET_CONFIG reply, got cmd={cmd:#04x}"
    return bytes(payload)


def set_config(ser, blob: bytes) -> None:
    cmd, _ = _txn(ser, CMD_SET_CONFIG, blob)
    assert cmd == CMD_SET_CONFIG, f"expected SET_CONFIG ack, got cmd={cmd:#04x}"


# ── subcommands ──────────────────────────────────────────────────────────────
def _print_config(cfg: dict) -> None:
    for i, p in enumerate(cfg["pages"]):
        print(f"  page {i + 1}: {p['name']!r}")
        for j, b in enumerate(p["buttons"]):
            act = b["on_press"]
            extra = "" if b["on_long_press"]["type"] == "none" \
                else f"  long={b['on_long_press']}"
            grp = f"  group={b['group']}" if b.get("group") else ""
            print(f"    [{j}] {b['label']!r:>8}  rgb{tuple(b['color'])}  {act}{extra}{grp}")
    thru = cfg.get("midi_thru", {})
    on = [f for f in THRU_FIELDS if thru.get(f)]
    print(f"  midi_thru: {', '.join(on) if on else 'none'}")
    for k, c in enumerate(cfg.get("cycles", [])):
        long = "" if c["long"] == "none" else f"  long={c['long']}"
        print(f"  cycle {k}: {len(c['steps'])} step(s){long}")
        for s in c["steps"]:
            print(f"      {s}")


def cmd_hello(ser, _args) -> None:
    print(f"OK - device HELLO, protocol v{hello(ser)}, seq echoed.")


def cmd_get(ser, args) -> None:
    blob = get_config(ser)
    cfg = decode_config(blob)
    print(f"OK - got config: {len(blob)} bytes, {len(cfg['pages'])} page(s).")
    _print_config(cfg)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(cfg, f, indent=2)
        print(f"  wrote JSON -> {args.json}")
    if args.raw:
        with open(args.raw, "wb") as f:
            f.write(blob)
        print(f"  wrote raw blob -> {args.raw}")


def cmd_set(ser, args) -> None:
    if args.file.endswith(".json"):
        with open(args.file, encoding="utf-8") as f:
            blob = encode_config(json.load(f))
    else:
        with open(args.file, "rb") as f:
            blob = f.read()
    set_config(ser, blob)
    print(f"OK - pushed {len(blob)} bytes; device persisted + hot-reloaded.")


def cmd_tweak(ser, args) -> None:
    cfg = decode_config(get_config(ser))
    page = cfg["pages"][0]
    page["name"] = args.name
    page["buttons"][0]["label"] = args.label
    page["buttons"][0]["color"] = [48, 0, 0]  # config::color::RED (L = 0x30)
    set_config(ser, encode_config(cfg))
    print(f"OK - page 1 renamed to {args.name!r}, button [0] -> {args.label!r}/red.")
    print("     Watch the screen title + first LED change live (RTT: "
          "'router: config applied').")


def cmd_roundtrip(ser, _args) -> None:
    before = get_config(ser)
    decode_config(before)  # sanity: it parses
    set_config(ser, before)
    after = get_config(ser)
    if before != after:
        raise SystemExit(f"FAIL - config changed across round-trip "
                         f"({len(before)} -> {len(after)} bytes)")
    print(f"OK - round-trip stable: {len(before)} bytes identical "
          "before/after GET->SET->GET.")


def main() -> None:
    ap = argparse.ArgumentParser(description="MIDI Captain config-sync CDC client")
    ap.add_argument("port", help="serial port (e.g. COM5)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("hello", help="probe the link")
    g = sub.add_parser("get", help="read + decode the device config")
    g.add_argument("--json", metavar="FILE", help="also save decoded config as JSON")
    g.add_argument("--raw", metavar="FILE", help="also save the raw postcard blob")
    s = sub.add_parser("set", help="push a config from FILE (.json or .bin)")
    s.add_argument("file", help="config file: .json (encoded) or .bin (raw blob)")
    t = sub.add_parser("tweak", help="rename page 1 + recolor a button, then push")
    t.add_argument("--name", default="REMOTE!", help="new page-1 name")
    t.add_argument("--label", default="HELLO", help="new label for page-1 button [0]")
    sub.add_parser("roundtrip", help="GET→SET→GET and assert unchanged")

    args = ap.parse_args()
    handlers = {
        "hello": cmd_hello, "get": cmd_get, "set": cmd_set,
        "tweak": cmd_tweak, "roundtrip": cmd_roundtrip,
    }
    with serial.Serial(args.port, 115200, timeout=2) as ser:
        handlers[args.cmd](ser, args)


if __name__ == "__main__":
    main()
