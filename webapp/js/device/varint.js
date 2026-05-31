// LEB128 unsigned varint — postcard's encoding for sequence/string lengths and
// enum discriminants. Mirrors `_enc_varint` / `_dec_varint` in the reference
// host client (firmware/scripts/cdc_config_client.py).
//
// Every value in the current RuntimeConfig fits a single byte (< 128), so this
// is byte-identical to a naive 1-byte length there — but we implement true
// multi-byte LEB128 so the codec stays correct if the wire format grows.

/** Append the LEB128 encoding of `value` to the number[] `out`. */
export function pushVarint(out, value) {
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`varint must be a non-negative integer: ${value}`);
  }
  let v = value;
  // Use division (not >>> 7) so values above 2^31 stay exact up to 2^53.
  do {
    let byte = v % 128;
    v = Math.floor(v / 128);
    if (v > 0) byte |= 0x80;
    out.push(byte);
  } while (v > 0);
}

/** Encode `value` to a Uint8Array. */
export function encodeVarint(value) {
  const out = [];
  pushVarint(out, value);
  return Uint8Array.from(out);
}

/**
 * Decode a LEB128 varint from `bytes` starting at `pos`.
 * @returns {[number, number]} `[value, nextPos]`
 */
export function decodeVarint(bytes, pos) {
  let result = 0;
  let mul = 1;
  let p = pos;
  for (;;) {
    if (p >= bytes.length) throw new RangeError("varint: truncated input");
    const byte = bytes[p++];
    result += (byte & 0x7f) * mul;
    if ((byte & 0x80) === 0) break;
    mul *= 128;
    if (mul > Number.MAX_SAFE_INTEGER) throw new RangeError("varint: too long");
  }
  return [result, p];
}
