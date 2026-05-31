#!/usr/bin/env python3
"""Generate golden test vectors for the JavaScript config-sync codec.

SELF-CONTAINED: this re-implements the device wire format (COBS + CRC-16 from
firmware/src/proto.rs, and the postcard RuntimeConfig layout from
firmware/src/config/mod.rs) directly in Python — no firmware checkout needed.
It is the independent second implementation that the JS codec in
webapp/js/device/ is checked against: webapp/test/codec.test.mjs asserts the JS
produces byte-identical output to the vectors emitted here.

Both this file and the JS codec mirror the SAME source of truth and should be
cross-validated against the hardware-verified host client
(firmware/scripts/cdc_config_client.py) / a real device when available. Keep all
three in lockstep on any wire change.

    python webapp/test/gen_golden.py            # writes webapp/test/golden_vectors.json

Wire layout (postcard):
  RuntimeConfig = varint(pages.len) ++ Page*
  Page          = String(name) ++ Button[10]            # fixed array, no len
  Button        = String(label) ++ u8 r ++ u8 g ++ u8 b ++ Action ++ Action
  String        = varint(byteLen) ++ utf8
  Action        = varint(tag) ++ fields:
    none(0) | midi_cc(1): u8 cc ++ CcValue | program_change(2): u8 program
    | sysex(3): varint(SysexCmd) ++ u8 param | page_change(4): u8 page
    | page_next(5) | page_prev(6) | tuner_toggle(7)
  CcValue       = varint(tag) [++ u8 value]   # fixed(0): u8 value | toggle(1)
  SysexCmd      = recall_preset(0)|amp_type(1)|gain(2)|volume(3)
"""
from __future__ import annotations

import argparse
import json
import os

PROTO_VERSION = 1
CMD_HELLO, CMD_GET_CONFIG, CMD_SET_CONFIG, CMD_REBOOT, CMD_ERROR = 0x01, 0x02, 0x03, 0x04, 0xFF

ACTION_TAG = {"none": 0, "midi_cc": 1, "program_change": 2, "sysex": 3,
              "page_change": 4, "page_next": 5, "page_prev": 6, "tuner_toggle": 7}
ACTION_NAME = {v: k for k, v in ACTION_TAG.items()}
CC_FIXED, CC_TOGGLE = 0, 1
SYSEX_TAG = {"recall_preset": 0, "amp_type": 1, "gain": 2, "volume": 3}
SYSEX_NAME = {v: k for k, v in SYSEX_TAG.items()}


# ── low-level codecs (mirror firmware/src/proto.rs) ─────────────────────────
def enc_varint(v: int) -> bytes:
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def dec_varint(data: bytes, pos: int):
    result, shift, p = 0, 0, pos
    while True:
        b = data[p]
        p += 1
        result |= (b & 0x7F) << shift
        if not (b & 0x80):
            return result, p
        shift += 7


def crc16(data: bytes) -> int:
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
            raise ValueError("bad COBS")
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


# ── postcard RuntimeConfig codec (mirror firmware/src/config/mod.rs) ─────────
def _enc_str(out: bytearray, s: str):
    b = s.encode("utf-8")
    out += enc_varint(len(b))
    out += b


def _enc_action(out: bytearray, a: dict):
    kind = a.get("kind", "none")
    out += enc_varint(ACTION_TAG[kind])
    if kind == "midi_cc":
        out.append(a["cc"] & 0xFF)
        if a.get("toggle"):
            out += enc_varint(CC_TOGGLE)
        else:
            out += enc_varint(CC_FIXED)
            out.append(a.get("value", 0) & 0xFF)
    elif kind == "program_change":
        out.append(a["program"] & 0xFF)
    elif kind == "sysex":
        out += enc_varint(SYSEX_TAG[a["sysex"]])
        out.append(a.get("param", 0) & 0xFF)
    elif kind == "page_change":
        out.append(a.get("page", 0) & 0xFF)


def _enc_button(out: bytearray, b: dict):
    _enc_str(out, b.get("label", ""))
    c = b.get("color", [0, 0, 0])
    out += bytes([c[0] & 0xFF, c[1] & 0xFF, c[2] & 0xFF])
    _enc_action(out, b.get("on_press", {"kind": "none"}))
    _enc_action(out, b.get("on_long_press", {"kind": "none"}))


def encode_config(cfg: dict) -> bytes:
    out = bytearray()
    pages = cfg.get("pages", [])
    out += enc_varint(len(pages))
    for p in pages:
        _enc_str(out, p.get("name", ""))
        buttons = p.get("buttons", [])
        for i in range(10):
            _enc_button(out, buttons[i] if i < len(buttons) else {})
    return bytes(out)


def _dec_str(data, i):
    n, p = dec_varint(data, i)
    return data[p:p + n].decode("utf-8"), p + n


def _dec_action(data, i):
    tag, p = dec_varint(data, i)
    kind = ACTION_NAME[tag]
    if kind == "midi_cc":
        cc = data[p]; p += 1
        mode, p = dec_varint(data, p)
        if mode == CC_TOGGLE:
            return {"kind": kind, "cc": cc, "toggle": True}, p
        value = data[p]; p += 1
        return {"kind": kind, "cc": cc, "value": value}, p
    if kind == "program_change":
        return {"kind": kind, "program": data[p]}, p + 1
    if kind == "sysex":
        sx, p = dec_varint(data, p)
        param = data[p]; p += 1
        return {"kind": kind, "sysex": SYSEX_NAME[sx], "param": param}, p
    if kind == "page_change":
        return {"kind": kind, "page": data[p]}, p + 1
    return {"kind": kind}, p


def _dec_button(data, i):
    label, p = _dec_str(data, i)
    color = [data[p], data[p + 1], data[p + 2]]
    p += 3
    on_press, p = _dec_action(data, p)
    on_long_press, p = _dec_action(data, p)
    return {"label": label, "color": color, "on_press": on_press, "on_long_press": on_long_press}, p


def decode_config(data: bytes):
    npages, p = dec_varint(data, 0)
    pages = []
    for _ in range(npages):
        name, p = _dec_str(data, p)
        buttons = []
        for _ in range(10):
            btn, p = _dec_button(data, p)
            buttons.append(btn)
        pages.append({"name": name, "buttons": buttons})
    return {"pages": pages}, p


# ── config builders (the dict shape the JS model also uses) ─────────────────
def midi_cc(cc, *, value=None, toggle=False):
    a = {"kind": "midi_cc", "cc": cc}
    if toggle:
        a["toggle"] = True
    else:
        a["value"] = value if value is not None else 0
    return a


def pc(program):
    return {"kind": "program_change", "program": program}


def sysex(name, param):
    return {"kind": "sysex", "sysex": name, "param": param}


def btn(label, color, on_press, on_long_press=None):
    return {"label": label, "color": list(color),
            "on_press": on_press or {"kind": "none"},
            "on_long_press": on_long_press or {"kind": "none"}}


EMPTY_BTN = {"label": "", "color": [0, 0, 0],
             "on_press": {"kind": "none"}, "on_long_press": {"kind": "none"}}


def page(name, buttons):
    bs = list(buttons) + [dict(EMPTY_BTN) for _ in range(10 - len(buttons))]
    return {"name": name, "buttons": bs[:10]}


L = 0x30
OFF, RED, GREEN, BLUE = [0, 0, 0], [L, 0, 0], [0, L, 0], [0, 0, L]
CYAN, AMBER, PURPLE, WHITE = [0, L, L], [L, L // 2, 0], [L // 2, 0, L], [L, L, L]


def firmware_default():
    p0 = page("Default", [
        btn("PRE1", WHITE, pc(0)), btn("PRE2", WHITE, pc(1)),
        btn("PRE3", WHITE, pc(2)), btn("PRE4", WHITE, pc(3)),
        btn("FX1", GREEN, midi_cc(80, toggle=True)),
        btn("FX2", BLUE, midi_cc(81, toggle=True)),
        btn("FX3", AMBER, midi_cc(82, toggle=True)),
        btn("FX4", PURPLE, midi_cc(83, toggle=True), {"kind": "tuner_toggle"}),
        btn("BANK+", CYAN, pc(4), {"kind": "page_next"}),
        btn("BANK-", CYAN, pc(5), {"kind": "page_prev"}),
    ])
    p1 = page("Katana", [
        btn("CLEAN", GREEN, sysex("amp_type", 1)),
        btn("CRUNCH", AMBER, sysex("amp_type", 2)),
        btn("LEAD", RED, sysex("amp_type", 3)),
        btn("BROWN", PURPLE, sysex("amp_type", 4), {"kind": "tuner_toggle"}),
        btn("CH1", BLUE, sysex("recall_preset", 1)),
        btn("CH2", BLUE, sysex("recall_preset", 2)),
        btn("CH3", BLUE, sysex("recall_preset", 3)),
        btn("CH4", BLUE, sysex("recall_preset", 4)),
        btn("PAGE+", CYAN, {"kind": "page_next"}),
        btn("PAGE-", CYAN, {"kind": "page_prev"}),
    ])
    return {"pages": [p0, p1]}


def build_configs():
    out = [("firmware_default", firmware_default()),
           ("one_empty_page", {"pages": [page("", [])]})]
    out.append(("all_actions", {"pages": [page("ALL", [
        btn("none", [1, 2, 3], {"kind": "none"}),
        btn("ccfix", [10, 20, 30], midi_cc(80, value=64), midi_cc(1, value=0)),
        btn("cctog", [11, 21, 31], midi_cc(80, toggle=True)),
        btn("pc", [40, 50, 60], pc(5)),
        btn("amp", [70, 80, 90], sysex("amp_type", 4), {"kind": "page_next"}),
        btn("rcl", OFF, sysex("recall_preset", 2)),
        btn("gain", OFF, sysex("gain", 100)),
        btn("vol", OFF, sysex("volume", 50)),
        btn("pgch", OFF, {"kind": "page_change", "page": 1}),
        btn("tune", WHITE, {"kind": "tuner_toggle"}, {"kind": "page_prev"}),
    ])]}))
    out.append(("multi_page", {"pages": [
        page("P1", [btn("A", RED, midi_cc(1, value=127))]),
        page("P2", [btn("B", GREEN, pc(9))]),
        page("P3", []),
    ]}))
    out.append(("edge_labels", {"pages": [page("page-name-16ch", [
        btn("", OFF, {"kind": "none"}),
        btn("ABCDEFGHIJKL", [1, 1, 1], {"kind": "none"}),   # 12 bytes
        btn("café", [2, 2, 2], {"kind": "none"}),        # 5 bytes
    ])]}))
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--client", default=None, help="ignored (self-contained); kept for CLI compat")
    ap.add_argument("--out", default=os.path.join("webapp", "test", "golden_vectors.json"))
    args = ap.parse_args(argv)

    varint_vectors = [{"value": v, "hex": enc_varint(v).hex()}
                      for v in [0, 1, 2, 63, 127, 128, 129, 255, 300, 16383, 16384, 2097151, 2097152]]

    cobs_cases = [b"", bytes([0]), bytes([0, 0]), bytes([1, 2, 3]), bytes([1, 0, 2]),
                  bytes([0, 1, 2, 0]), bytes(range(1, 255)), bytes([1] * 255), bytes([5, 0])]
    cobs_vectors = []
    for c in cobs_cases:
        enc = cobs_encode(c)
        assert cobs_decode(enc) == c, c.hex()
        cobs_vectors.append({"decoded_hex": c.hex(), "encoded_hex": enc.hex()})

    crc_cases = [b"", b"\x01\x00", b"123456789", bytes([0x02, 0x00]), bytes(range(16))]
    crc_vectors = [{"input_hex": c.hex(), "crc": crc16(c)} for c in crc_cases]
    assert crc16(b"123456789") == 0x29B1, "CRC self-check failed"

    configs = []
    for name, cfg in build_configs():
        blob = encode_config(cfg)
        decoded, consumed = decode_config(blob)
        assert consumed == len(blob), f"{name}: trailing bytes"
        assert encode_config(decoded) == blob, f"{name}: round-trip differs"
        configs.append({"name": name, "config": cfg, "postcard_hex": blob.hex()})

    cfg_by = {c["name"]: bytes.fromhex(c["postcard_hex"]) for c in configs}
    frame_vectors = [
        {"desc": "hello_req", "cmd": CMD_HELLO, "seq": 0x42, "payload_hex": "01",
         "frame_hex": encode_frame(CMD_HELLO, 0x42, bytes([1])).hex()},
        {"desc": "hello_resp", "cmd": CMD_HELLO, "seq": 0x42, "payload_hex": "01",
         "frame_hex": encode_frame(CMD_HELLO, 0x42, bytes([1])).hex()},
        {"desc": "get_req", "cmd": CMD_GET_CONFIG, "seq": 0x01, "payload_hex": "",
         "frame_hex": encode_frame(CMD_GET_CONFIG, 0x01, b"").hex()},
        {"desc": "get_resp_firmware_default", "cmd": CMD_GET_CONFIG, "seq": 0x01,
         "payload_hex": cfg_by["firmware_default"].hex(),
         "frame_hex": encode_frame(CMD_GET_CONFIG, 0x01, cfg_by["firmware_default"]).hex()},
        {"desc": "set_req_all_actions", "cmd": CMD_SET_CONFIG, "seq": 0x02,
         "payload_hex": cfg_by["all_actions"].hex(),
         "frame_hex": encode_frame(CMD_SET_CONFIG, 0x02, cfg_by["all_actions"]).hex()},
        {"desc": "error_badcommand", "cmd": CMD_ERROR, "seq": 0x00, "payload_hex": "01",
         "frame_hex": encode_frame(CMD_ERROR, 0x00, bytes([1])).hex()},
    ]

    doc = {
        "_provenance": ("Generated by webapp/test/gen_golden.py (self-contained, mirrors "
                        "firmware/src/config/mod.rs + proto.rs). Cross-validate against "
                        "firmware/scripts/cdc_config_client.py / hardware on any wire change."),
        "proto_version": PROTO_VERSION,
        "opcodes": {"HELLO": CMD_HELLO, "GET_CONFIG": CMD_GET_CONFIG,
                    "SET_CONFIG": CMD_SET_CONFIG, "REBOOT": CMD_REBOOT, "ERROR": CMD_ERROR},
        "varint_vectors": varint_vectors,
        "cobs_vectors": cobs_vectors,
        "crc_vectors": crc_vectors,
        "frame_vectors": frame_vectors,
        "configs": configs,
    }
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=2, ensure_ascii=True)
        f.write("\n")
    print(f"wrote {args.out}: {len(configs)} configs, {len(cobs_vectors)} cobs, "
          f"{len(crc_vectors)} crc, {len(frame_vectors)} frames, {len(varint_vectors)} varints")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
