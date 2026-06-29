// Node self-test for the config-sync codec. Asserts the JS codec in
// webapp/js/device/ produces byte-identical output to the hardware-verified
// reference (captured in golden_vectors.json by gen_golden.py).
//
//   node test/codec.test.mjs      (from webapp/)  — or:  npm test
//
// A short PASS/FAIL summary is printed last. Exit code is non-zero on failure.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { encodeVarint, decodeVarint } from "../js/device/varint.js";
import { crc16 } from "../js/device/crc16.js";
import { cobsEncode, cobsDecode } from "../js/device/cobs.js";
import { encodeFrame, decodeFrame } from "../js/device/frame.js";
import { encodeConfig, decodeConfig } from "../js/device/postcard.js";

const here = dirname(fileURLToPath(import.meta.url));
const golden = JSON.parse(readFileSync(join(here, "golden_vectors.json"), "utf-8"));

const hex = (u8) => Buffer.from(u8).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));

let pass = 0;
const failures = [];
function check(name, cond, detail = "") {
  if (cond) pass++;
  else failures.push(`${name}${detail ? `: ${detail}` : ""}`);
}
function eqBytes(name, got, wantHex) {
  check(name, hex(got) === wantHex, `got ${hex(got)} want ${wantHex}`);
}
function deepEqual(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

// ── varint ──
for (const v of golden.varint_vectors) {
  eqBytes(`varint encode ${v.value}`, encodeVarint(v.value), v.hex);
  const [val, pos] = decodeVarint(fromHex(v.hex), 0);
  check(`varint decode ${v.value}`, val === v.value && pos === v.hex.length / 2,
    `got ${val}@${pos}`);
}

// ── CRC-16 ──
for (const c of golden.crc_vectors) {
  check(`crc16 ${c.input_hex || "(empty)"}`, crc16(fromHex(c.input_hex)) === c.crc,
    `got 0x${crc16(fromHex(c.input_hex)).toString(16)} want 0x${c.crc.toString(16)}`);
}

// ── COBS ──
for (const c of golden.cobs_vectors) {
  eqBytes(`cobs encode ${c.decoded_hex || "(empty)"}`, cobsEncode(fromHex(c.decoded_hex)), c.encoded_hex);
  eqBytes(`cobs decode ${c.encoded_hex}`, cobsDecode(fromHex(c.encoded_hex)), c.decoded_hex);
}

// ── frames ──
for (const f of golden.frame_vectors) {
  eqBytes(`frame encode ${f.desc}`, encodeFrame(f.cmd, f.seq, fromHex(f.payload_hex)), f.frame_hex);
  // decodeFrame takes the COBS bytes before the 0x00 delimiter.
  const cobsOnly = fromHex(f.frame_hex).slice(0, -1);
  const d = decodeFrame(cobsOnly);
  check(`frame decode ${f.desc}`,
    d.cmd === f.cmd && d.seq === f.seq && hex(d.payload) === f.payload_hex,
    `got cmd=${d.cmd} seq=${d.seq} payload=${hex(d.payload)}`);
}

// ── configs (postcard) ──
for (const c of golden.configs) {
  eqBytes(`config encode ${c.name}`, encodeConfig(c.config), c.postcard_hex);
  const { config: decoded, consumed } = decodeConfig(fromHex(c.postcard_hex));
  check(`config decode-consumes-all ${c.name}`, consumed === c.postcard_hex.length / 2,
    `consumed ${consumed}/${c.postcard_hex.length / 2}`);
  check(`config decode-deep-equals ${c.name}`, deepEqual(decoded, c.config));
  eqBytes(`config round-trip ${c.name}`, encodeConfig(decoded), c.postcard_hex);
}

// ── report ──
const total = pass + failures.length;
if (failures.length) {
  console.error(`FAIL ${failures.length}/${total}`);
  for (const f of failures.slice(0, 20)) console.error("  - " + f);
  process.exit(1);
}
console.log(`PASS ${pass}/${total} codec vectors (proto v${golden.proto_version})`);
