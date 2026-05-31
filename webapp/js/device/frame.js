// Frame codec: body = cmd(1) | seq(1) | payload(N) | crc16(2, big-endian);
// the CRC covers cmd|seq|payload. On the wire: COBS(body) || 0x00.
// Mirrors `encode_frame` / `decode_frame` in the reference host client and
// `proto::encode` / `proto::decode` in firmware.

import { crc16 } from "./crc16.js";
import { cobsEncode, cobsDecode } from "./cobs.js";

/** Frame delimiter on the wire. */
export const DELIMITER = 0x00;

/** Command opcodes (firmware/src/proto.rs `cmd`). */
export const CMD = Object.freeze({
  HELLO: 0x01,
  GET_CONFIG: 0x02,
  SET_CONFIG: 0x03,
  REBOOT: 0x04,
  ERROR: 0xff,
});

/** Application error codes carried in an ERROR reply (proto::ProtoError). */
export const PROTO_ERROR = Object.freeze({
  1: "BadCommand",
  2: "BadPayload",
  3: "StoreFailed",
});

/**
 * Build a complete frame (COBS body + trailing 0x00) for transmission.
 * @param {number} cmd
 * @param {number} seq
 * @param {Uint8Array | number[]} [payload]
 * @returns {Uint8Array}
 */
export function encodeFrame(cmd, seq, payload = new Uint8Array(0)) {
  const body = new Uint8Array(2 + payload.length + 2);
  body[0] = cmd & 0xff;
  body[1] = seq & 0xff;
  body.set(payload, 2);
  const crc = crc16(body.subarray(0, 2 + payload.length));
  body[2 + payload.length] = (crc >> 8) & 0xff;
  body[3 + payload.length] = crc & 0xff;

  const cobs = cobsEncode(body);
  const frame = new Uint8Array(cobs.length + 1);
  frame.set(cobs, 0);
  frame[cobs.length] = DELIMITER;
  return frame;
}

/**
 * Decode a received frame (the COBS bytes BEFORE the 0x00 delimiter).
 * Verifies COBS structure and CRC.
 * @param {Uint8Array | number[]} cobsBytes
 * @returns {{cmd: number, seq: number, payload: Uint8Array}}
 */
export function decodeFrame(cobsBytes) {
  const body = cobsDecode(cobsBytes);
  if (body.length < 4) throw new Error("frame: too short");
  const crcPos = body.length - 2;
  const want = (body[crcPos] << 8) | body[crcPos + 1];
  const got = crc16(body.subarray(0, crcPos));
  if (got !== want) {
    throw new Error(
      `frame: CRC mismatch (got 0x${got.toString(16)}, want 0x${want.toString(16)})`,
    );
  }
  return { cmd: body[0], seq: body[1], payload: body.subarray(2, crcPos) };
}
