use crate::protocol::ProtocolError;

pub const CHUNK_MAGIC: u8 = 0x03;
pub const CHUNK_ACK_MAGIC: u8 = 0x04;

pub const FLAG_FIRST: u8 = 0x01;
pub const FLAG_LAST: u8 = 0x02;
pub const FLAG_NEEDS_ACK: u8 = 0x04;
pub const FLAG_ENCRYPTED: u8 = 0x08;

/// A fully reassembled Zepp message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeppMessage {
    pub endpoint: u16,
    pub payload: Vec<u8>,
    /// True when the band sent this message under the session key.
    pub encrypted: bool,
}

/// The acknowledgement the band asked us to send back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    pub handle: u8,
    pub count: u8,
}

impl Ack {
    /// The band expects these five bytes on the read characteristic.
    pub fn to_bytes(self) -> [u8; 5] {
        [CHUNK_ACK_MAGIC, 0x00, self.handle, 0x01, self.count]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodeOutcome {
    pub message: Option<ZeppMessage>,
    pub ack: Option<Ack>,
}

/// Buffer size the band uses for an encrypted body: payload, 4-byte sequence,
/// 4-byte CRC32, padded up to a 16-byte AES block.
pub fn encrypted_capacity(plain_len: usize) -> usize {
    let mut total = plain_len + 8;
    let overflow = total % 16;
    if overflow > 0 {
        total += 16 - overflow;
    }
    total
}

struct Reassembly {
    handle: u8,
    endpoint: u16,
    plain_len: usize,
    capacity: usize,
    encrypted: bool,
    buf: Vec<u8>,
}

pub struct ChunkDecoder {
    extended_header: bool,
    current: Option<Reassembly>,
    session_key: Option<[u8; 16]>,
}

impl ChunkDecoder {
    pub fn new(extended_header: bool) -> Self {
        Self {
            extended_header,
            current: None,
            session_key: None,
        }
    }

    pub fn set_session_key(&mut self, key: [u8; 16]) {
        self.session_key = Some(key);
    }

    pub fn push(&mut self, packet: &[u8]) -> Result<DecodeOutcome, ProtocolError> {
        if packet.is_empty() {
            return Err(ProtocolError::Truncated);
        }
        if packet[0] == CHUNK_ACK_MAGIC {
            // The band is acknowledging one of our writes. Nothing to do.
            return Ok(DecodeOutcome::default());
        }
        if packet[0] != CHUNK_MAGIC {
            return Err(ProtocolError::BadMagic(packet[0]));
        }

        let mut i = 1usize;
        let flags = *packet.get(i).ok_or(ProtocolError::Truncated)?;
        i += 1;
        if self.extended_header {
            i += 1; // extended header byte, always zero today
        }
        let handle = *packet.get(i).ok_or(ProtocolError::Truncated)?;
        i += 1;
        let count = *packet.get(i).ok_or(ProtocolError::Truncated)?;
        i += 1;

        let first = flags & FLAG_FIRST != 0;
        let last = flags & FLAG_LAST != 0;
        let needs_ack = flags & FLAG_NEEDS_ACK != 0;
        let encrypted = flags & FLAG_ENCRYPTED != 0;

        if first {
            let header = packet.get(i..i + 6).ok_or(ProtocolError::Truncated)?;
            let plain_len =
                u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
            let endpoint = u16::from_le_bytes([header[4], header[5]]);
            i += 6;

            let capacity = if encrypted {
                encrypted_capacity(plain_len)
            } else {
                plain_len
            };

            self.current = Some(Reassembly {
                handle,
                endpoint,
                plain_len,
                capacity,
                encrypted,
                buf: Vec::with_capacity(capacity),
            });
        }

        let state = self
            .current
            .as_mut()
            .ok_or(ProtocolError::UnexpectedContinuation)?;
        if state.handle != handle {
            let expected = state.handle;
            self.current = None;
            return Err(ProtocolError::HandleMismatch {
                expected,
                actual: handle,
            });
        }
        state.buf.extend_from_slice(&packet[i..]);

        let ack = needs_ack.then_some(Ack { handle, count });
        if !last {
            return Ok(DecodeOutcome { message: None, ack });
        }

        let state = self
            .current
            .take()
            .expect("reassembly state was just checked");
        let payload = self.finish(state)?;
        Ok(DecodeOutcome {
            message: Some(payload),
            ack,
        })
    }

    fn finish(&self, mut state: Reassembly) -> Result<ZeppMessage, ProtocolError> {
        if state.encrypted {
            let key = self.session_key.ok_or(ProtocolError::NoSessionKey)?;
            let message_key = message_key(&key, state.handle);
            state.buf.resize(state.capacity, 0);
            let clear = crate::crypto::aes_ecb_decrypt(&message_key, &state.buf)?;
            state.buf = clear;
        }
        state.buf.truncate(state.plain_len);
        Ok(ZeppMessage {
            endpoint: state.endpoint,
            payload: state.buf,
            encrypted: state.encrypted,
        })
    }
}

/// Per-message key: every byte of the session key XORed with the chunk handle.
pub fn message_key(session_key: &[u8; 16], handle: u8) -> [u8; 16] {
    let mut key = [0u8; 16];
    for i in 0..16 {
        key[i] = session_key[i] ^ handle;
    }
    key
}

pub struct ChunkEncoder {
    mtu: u16,
    handle: u8,
    session_key: Option<[u8; 16]>,
    sequence: u32,
}

impl ChunkEncoder {
    pub fn new(mtu: u16) -> Self {
        Self {
            mtu,
            handle: 0,
            session_key: None,
            sequence: 0,
        }
    }

    pub fn set_encryption(&mut self, sequence: u32, key: [u8; 16]) {
        self.sequence = sequence;
        self.session_key = Some(key);
    }

    pub fn encode(
        &mut self,
        endpoint: u16,
        payload: &[u8],
        extended: bool,
        encrypt: bool,
    ) -> Result<Vec<Vec<u8>>, ProtocolError> {
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }

        let first_header = if extended { 11 } else { 10 };
        let next_header = if extended { 5 } else { 4 };
        if (self.mtu as usize) <= first_header + 3 {
            return Err(ProtocolError::MtuTooSmall(self.mtu));
        }

        self.handle = self.handle.wrapping_add(1);
        let handle = self.handle;
        let plain_len = payload.len();

        let body = if encrypt {
            let session_key = self.session_key.ok_or(ProtocolError::NoSessionKey)?;
            self.seal(&session_key, handle, payload)?
        } else {
            payload.to_vec()
        };

        let mut chunks = Vec::new();
        let mut sent = 0usize;
        let mut count: u8 = 0;
        let mut header_size = first_header;

        while sent < body.len() {
            let max_chunk = self.mtu as usize - 3 - header_size;
            let copy = (body.len() - sent).min(max_chunk);
            let mut chunk = vec![0u8; header_size + copy];

            let mut flags = 0u8;
            if encrypt {
                flags |= FLAG_ENCRYPTED;
            }
            if count == 0 {
                flags |= FLAG_FIRST;
                let base = if extended { 5 } else { 4 };
                chunk[base..base + 4].copy_from_slice(&(plain_len as u32).to_le_bytes());
                chunk[base + 4..base + 6].copy_from_slice(&endpoint.to_le_bytes());
            }
            if body.len() - sent <= max_chunk {
                flags |= FLAG_LAST | FLAG_NEEDS_ACK;
            }

            chunk[0] = CHUNK_MAGIC;
            chunk[1] = flags;
            if extended {
                chunk[2] = 0x00;
                chunk[3] = handle;
                chunk[4] = count;
            } else {
                chunk[2] = handle;
                chunk[3] = count;
            }
            chunk[header_size..].copy_from_slice(&body[sent..sent + copy]);

            chunks.push(chunk);
            sent += copy;
            count = count.wrapping_add(1);
            header_size = next_header;
        }

        Ok(chunks)
    }

    /// Append the sequence number and CRC32, pad to a block, then encrypt.
    fn seal(
        &mut self,
        session_key: &[u8; 16],
        handle: u8,
        payload: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut buf = vec![0u8; encrypted_capacity(payload.len())];
        let len = payload.len();
        buf[..len].copy_from_slice(payload);
        buf[len..len + 4].copy_from_slice(&self.sequence.to_le_bytes());
        self.sequence = self.sequence.wrapping_add(1);

        let checksum = crc32fast::hash(&buf[..len + 4]);
        buf[len + 4..len + 8].copy_from_slice(&checksum.to_le_bytes());

        let key = message_key(session_key, handle);
        crate::crypto::aes_ecb_encrypt(&key, &buf)
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;

    fn decoder() -> ChunkDecoder {
        ChunkDecoder::new(true)
    }

    #[test]
    fn decodes_a_single_plaintext_chunk() {
        // extended header, first+last, handle 7, count 0, len 2, endpoint 0x001b, payload 0505
        let packet = hex::decode("0303000700020000001b000505").unwrap();
        let out = decoder().push(&packet).unwrap();
        let msg = out.message.expect("expected a message");
        assert_eq!(msg.endpoint, 0x001b);
        assert_eq!(msg.payload, vec![0x05, 0x05]);
        assert!(!msg.encrypted);
        assert_eq!(out.ack, None);
    }

    #[test]
    fn requests_an_ack_when_the_flag_is_set() {
        let packet = hex::decode("0307000700020000001b000505").unwrap();
        let out = decoder().push(&packet).unwrap();
        assert!(out.message.is_some());
        assert_eq!(out.ack, Some(Ack { handle: 7, count: 0 }));
        assert_eq!(
            Ack { handle: 7, count: 0 }.to_bytes(),
            [0x04, 0x00, 0x07, 0x01, 0x00]
        );
    }

    #[test]
    fn reassembles_two_chunks() {
        let mut d = decoder();
        // first chunk: len 6, endpoint 0x0082, payload aabbcc
        let first = hex::decode("0301000900060000008200aabbcc").unwrap();
        let out = d.push(&first).unwrap();
        assert!(out.message.is_none());

        // continuation and last: payload ddeeff
        let last = hex::decode("0302000901ddeeff").unwrap();
        let out = d.push(&last).unwrap();
        let msg = out.message.expect("expected a message");
        assert_eq!(msg.endpoint, 0x0082);
        assert_eq!(msg.payload, vec![0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    }

    #[test]
    fn ignores_band_acknowledgements() {
        let out = decoder().push(&[0x04, 0x00, 0x01, 0x01, 0x00]).unwrap();
        assert_eq!(out, DecodeOutcome::default());
    }

    #[test]
    fn rejects_an_unknown_magic_byte() {
        assert!(decoder().push(&[0x09, 0x00]).is_err());
    }

    #[test]
    fn rejects_a_truncated_header() {
        assert!(decoder().push(&[0x03, 0x03]).is_err());
    }

    #[test]
    fn drops_a_chunk_with_a_foreign_handle() {
        let mut d = decoder();
        let first = hex::decode("0301000900060000008200aabbcc").unwrap();
        d.push(&first).unwrap();
        let foreign = hex::decode("0302000a01ddeeff").unwrap();
        assert!(d.push(&foreign).is_err());
    }

    #[test]
    fn sizes_the_encrypted_buffer_to_a_block_boundary() {
        // plaintext 2 bytes -> 2 + 8 = 10 -> rounded to 16
        assert_eq!(encrypted_capacity(2), 16);
        // plaintext 8 bytes -> 16 exactly
        assert_eq!(encrypted_capacity(8), 16);
        // plaintext 9 bytes -> 17 -> rounded to 32
        assert_eq!(encrypted_capacity(9), 32);
    }
}

#[cfg(test)]
mod encoder_tests {
    use super::*;

    #[test]
    fn encodes_the_auth_public_key_in_one_chunk() {
        let mut enc = ChunkEncoder::new(247);
        let mut payload = vec![0x04, 0x02, 0x00, 0x02];
        payload.extend_from_slice(&[0xaa; 48]);
        assert_eq!(payload.len(), 52);

        let chunks = enc.encode(0x0082, &payload, true, false).unwrap();
        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert_eq!(c.len(), 52 + 11);
        assert_eq!(c[0], CHUNK_MAGIC);
        assert_eq!(c[1], FLAG_FIRST | FLAG_LAST | FLAG_NEEDS_ACK);
        assert_eq!(c[2], 0x00);
        assert_eq!(c[3], 1, "the first message must use handle 1");
        assert_eq!(c[4], 0, "count starts at zero");
        assert_eq!(&c[5..9], &52u32.to_le_bytes());
        assert_eq!(&c[9..11], &0x0082u16.to_le_bytes());
        assert_eq!(&c[11..], &payload[..]);
    }

    #[test]
    fn splits_across_chunks_at_the_minimum_mtu() {
        let mut enc = ChunkEncoder::new(23);
        let payload = vec![0x11u8; 52];
        let chunks = enc.encode(0x0082, &payload, true, false).unwrap();

        // first chunk carries 23 - 3 - 11 = 9 bytes, the rest carry 23 - 3 - 5 = 15
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].len(), 11 + 9);
        assert_eq!(chunks[1].len(), 5 + 15);
        assert_eq!(chunks[2].len(), 5 + 15);
        assert_eq!(chunks[3].len(), 5 + 13);

        assert_eq!(chunks[0][1], FLAG_FIRST);
        assert_eq!(chunks[1][1], 0);
        assert_eq!(chunks[2][1], 0);
        assert_eq!(chunks[3][1], FLAG_LAST | FLAG_NEEDS_ACK);

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk[3], 1, "handle must stay constant");
            assert_eq!(chunk[4], i as u8, "count must increment");
        }
    }

    #[test]
    fn increments_the_handle_per_message() {
        let mut enc = ChunkEncoder::new(247);
        let a = enc.encode(0x0082, &[0x01], true, false).unwrap();
        let b = enc.encode(0x0082, &[0x01], true, false).unwrap();
        assert_eq!(a[0][3], 1);
        assert_eq!(b[0][3], 2);
    }

    #[test]
    fn encrypted_output_round_trips_through_the_decoder() {
        let session_key = [0x5au8; 16];
        let mut enc = ChunkEncoder::new(247);
        enc.set_encryption(0x11223344, session_key);

        let payload = vec![0xde, 0xad, 0xbe, 0xef];
        let chunks = enc.encode(0x001b, &payload, true, true).unwrap();
        assert_eq!(chunks[0][1] & FLAG_ENCRYPTED, FLAG_ENCRYPTED);
        // 4 bytes payload + 4 sequence + 4 crc = 12, padded to 16
        assert_eq!(chunks[0].len(), 11 + 16);

        let mut dec = ChunkDecoder::new(true);
        dec.set_session_key(session_key);
        let out = dec.push(&chunks[0]).unwrap();
        assert_eq!(out.message.unwrap().payload, payload);
    }

    #[test]
    fn rejects_an_empty_payload() {
        let mut enc = ChunkEncoder::new(247);
        assert!(enc.encode(0x0082, &[], true, false).is_err());
    }

    #[test]
    fn rejects_an_mtu_that_cannot_carry_a_chunk() {
        let mut enc = ChunkEncoder::new(14);
        assert!(enc.encode(0x0082, &[0x01], true, false).is_err());
    }

    #[test]
    fn refuses_to_encrypt_without_a_session_key() {
        let mut enc = ChunkEncoder::new(247);
        assert!(enc.encode(0x001b, &[0x01], true, true).is_err());
    }
}
