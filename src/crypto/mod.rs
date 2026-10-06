pub mod b163;

use crate::protocol::ProtocolError;
use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

/// AES-128 ECB on exactly one block. The Zepp handshake needs this shape.
pub fn aes_ecb_encrypt_block(key: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let cipher = Aes128::new(&Array::from(*key));
    let mut b = Array::from(*block);
    cipher.encrypt_block(&mut b);
    b.into()
}

pub fn aes_ecb_decrypt_block(key: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let cipher = Aes128::new(&Array::from(*key));
    let mut b = Array::from(*block);
    cipher.decrypt_block(&mut b);
    b.into()
}

pub fn aes_ecb_encrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(ProtocolError::BadBlockLength(data.len()));
    }
    let mut out = data.to_vec();
    for block in out.chunks_exact_mut(16) {
        let input = <[u8; 16]>::try_from(&block[..]).expect("chunk is 16 bytes");
        block.copy_from_slice(&aes_ecb_encrypt_block(key, &input));
    }
    Ok(out)
}

pub fn aes_ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(ProtocolError::BadBlockLength(data.len()));
    }
    let mut out = data.to_vec();
    for block in out.chunks_exact_mut(16) {
        let input = <[u8; 16]>::try_from(&block[..]).expect("chunk is 16 bytes");
        block.copy_from_slice(&aes_ecb_decrypt_block(key, &input));
    }
    Ok(out)
}

/// session_key[i] = shared[i + 8] XOR auth_key[i]
pub fn derive_session_key(shared: &[u8], auth_key: &[u8; 16]) -> Result<[u8; 16], ProtocolError> {
    let slice = shared
        .get(8..24)
        .ok_or(ProtocolError::MalformedAuthResponse)?;
    let mut key = [0u8; 16];
    for i in 0..16 {
        key[i] = slice[i] ^ auth_key[i];
    }
    Ok(key)
}

/// The outgoing encrypted sequence number is the first four shared bytes, little-endian.
pub fn derive_sequence_number(shared: &[u8]) -> Result<u32, ProtocolError> {
    let slice = shared.get(0..4).ok_or(ProtocolError::MalformedAuthResponse)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIPS_KEY: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    const FIPS_PLAIN: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];

    #[test]
    fn matches_the_fips_197_vector() {
        let out = aes_ecb_encrypt_block(&FIPS_KEY, &FIPS_PLAIN);
        assert_eq!(hex::encode(out), "69c4e0d86a7b0430d8cdb78070b4c55a");
    }

    #[test]
    fn decrypt_reverses_encrypt() {
        let cipher = aes_ecb_encrypt(&FIPS_KEY, &FIPS_PLAIN).unwrap();
        let clear = aes_ecb_decrypt(&FIPS_KEY, &cipher).unwrap();
        assert_eq!(clear, FIPS_PLAIN.to_vec());
    }

    #[test]
    fn rejects_a_partial_block() {
        assert!(aes_ecb_encrypt(&FIPS_KEY, &[0u8; 17]).is_err());
        assert!(aes_ecb_decrypt(&FIPS_KEY, &[0u8; 3]).is_err());
    }

    // Vector derived from the Gadgetbridge test key pair. See Task 7.
    const SHARED: &str = "119952464b0ff4f0ce59dc012af77a00502efb5e060000003479ea6b37fa4edb7fdaf4c0c2df3ea5a637f4ac06000000";

    #[test]
    fn derives_the_session_key_from_the_shared_secret() {
        let shared = hex::decode(SHARED).unwrap();
        let key = derive_session_key(&shared, &FIPS_KEY).unwrap();
        assert_eq!(hex::encode(key), "ce58de022ef27c075827f1550a0d0e0f");
    }

    #[test]
    fn derives_the_sequence_number_from_the_shared_secret() {
        let shared = hex::decode(SHARED).unwrap();
        assert_eq!(derive_sequence_number(&shared).unwrap(), 0x4652_9911);
    }

    #[test]
    fn rejects_a_short_shared_secret() {
        assert!(derive_session_key(&[0u8; 10], &FIPS_KEY).is_err());
        assert!(derive_sequence_number(&[0u8; 2]).is_err());
    }
}
