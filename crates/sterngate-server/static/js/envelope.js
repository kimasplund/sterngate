/**
 * Sterngate Zero-Trust Command Envelope & IEEE 802.3 CRC32 Generator
 */
function makeCrcTable() {
  let c;
  const table = [];
  for (let n = 0; n < 256; n++) {
    c = n;
    for (let k = 0; k < 8; k++) {
      c = ((c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1));
    }
    table[n] = c;
  }
  return table;
}

const CRC_TABLE = makeCrcTable();

function crc32(bytes) {
  let crc = 0 ^ (-1);
  for (let i = 0; i < bytes.length; i++) {
    crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ bytes[i]) & 0xFF];
  }
  return (crc ^ (-1)) >>> 0;
}

function createCommandEnvelope(targetModule, service, did, payloadBytes) {
  return {
    command_id: (typeof crypto !== 'undefined' && crypto.randomUUID) ? crypto.randomUUID() : 'cmd-' + Date.now(),
    timestamp_ms: Date.now(),
    ttl_ms: 10000,
    idempotency_key: (typeof crypto !== 'undefined' && crypto.randomUUID) ? crypto.randomUUID() : 'idemp-' + Math.random(),
    target_module: targetModule,
    service: service,
    did: did,
    payload_len: payloadBytes.length,
    payload_crc32: crc32(payloadBytes),
    payload: Array.from(payloadBytes)
  };
}
