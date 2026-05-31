// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflection, no xorout.
// Check value for "123456789" is 0x29B1. Byte-for-byte the same routine as
// `crc16()` in firmware/src/proto.rs and the reference host client.

/**
 * @param {Uint8Array | number[]} data
 * @returns {number} 16-bit CRC
 */
export function crc16(data) {
  let crc = 0xffff;
  for (let i = 0; i < data.length; i++) {
    crc ^= (data[i] & 0xff) << 8;
    for (let b = 0; b < 8; b++) {
      crc = crc & 0x8000 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
    }
  }
  return crc & 0xffff;
}
