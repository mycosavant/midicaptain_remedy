// COBS (Consistent Overhead Byte Stuffing) — removes every 0x00 from a frame
// body so a single 0x00 can delimit frames on the wire. Identical algorithm to
// `cobs_encode` / `cobs_decode` in firmware/src/proto.rs (and the reference
// host client), including the redundant trailing 0x01 the encoder emits after
// an exact 254-byte run — both sides' decoders accept it.

/**
 * COBS-encode `data` (no delimiter appended).
 * @param {Uint8Array | number[]} data
 * @returns {Uint8Array}
 */
export function cobsEncode(data) {
  const out = [0]; // out[0] reserved for the first block's code byte
  let codePos = 0;
  let code = 1;
  for (let i = 0; i < data.length; i++) {
    const b = data[i] & 0xff;
    if (b === 0) {
      out[codePos] = code;
      codePos = out.length;
      out.push(0); // reserve next code byte
      code = 1;
    } else {
      out.push(b);
      code++;
      if (code === 0xff) {
        out[codePos] = code;
        codePos = out.length;
        out.push(0);
        code = 1;
      }
    }
  }
  out[codePos] = code;
  return Uint8Array.from(out);
}

/**
 * COBS-decode `data` (the bytes before the 0x00 delimiter).
 * @param {Uint8Array | number[]} data
 * @returns {Uint8Array}
 */
export function cobsDecode(data) {
  const out = [];
  let i = 0;
  while (i < data.length) {
    const code = data[i++] & 0xff;
    if (code === 0) throw new Error("COBS: interior zero byte");
    for (let j = 1; j < code; j++) {
      if (i >= data.length) throw new Error("COBS: block overruns input");
      out.push(data[i++] & 0xff);
    }
    // A non-0xFF block that is not the last block implies a trailing zero.
    if (code !== 0xff && i < data.length) out.push(0);
  }
  return Uint8Array.from(out);
}
