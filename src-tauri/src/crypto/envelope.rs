use crate::crypto::{CryptoError, DomainKey};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

// XChaCha20-Poly1305 AEAD envelope per CLAUDE.md "Resolved during
// Phase 1 §2". The 24-byte nonce makes random nonce generation safe
// for the operator's lifetime — no per-key message counter to track,
// no ~2^32 birthday-bound to budget. Cost is 12 extra bytes per bundle
// vs. plain ChaCha20-Poly1305, which is noise against any realistic
// plaintext.
//
// Bundle layout:
//   [0..4)        magic           = b"COOE"
//   [4]           bundle version  = 0x01
//   [5]           aead id         = 0x02 (XChaCha20-Poly1305)
//   [6..30)       nonce           (24 bytes, random per call)
//   [30..)        ciphertext || 16-byte Poly1305 tag
//
// The 6-byte header is bound as AAD on both encrypt and decrypt. This
// kills downgrade-style attacks that would swap leading bytes between
// formats, and establishes the AAD plumbing so §3 can add semantic AAD
// (row id, etc.) as a v2 bundle bump rather than a public-API change.
//
// Decrypt failure mode is uniform: framing checks (length, magic,
// version, aead id) yield CryptoError::MalformedBundle with a
// &'static str reason; everything else collapses to CryptoError::Aead.
// We do not differentiate "tag mismatch" from "wrong key" publicly —
// from the caller's perspective, the bundle did not authenticate.

const BUNDLE_MAGIC: [u8; 4] = *b"COOE";
const BUNDLE_VERSION: u8 = 0x01;
const AEAD_ID_XCHACHA20POLY1305: u8 = 0x02;

const HEADER_LEN: usize = 6;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const MIN_BUNDLE_LEN: usize = HEADER_LEN + NONCE_LEN + TAG_LEN; // = 46

fn build_header() -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&BUNDLE_MAGIC);
    h[4] = BUNDLE_VERSION;
    h[5] = AEAD_ID_XCHACHA20POLY1305;
    h
}

pub fn encrypt(key: &DomainKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce)?;
    encrypt_inner(key, &nonce, plaintext)
}

pub fn decrypt(key: &DomainKey, bundle: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if bundle.len() < MIN_BUNDLE_LEN {
        return Err(CryptoError::MalformedBundle("too short"));
    }
    if bundle[0..4] != BUNDLE_MAGIC {
        return Err(CryptoError::MalformedBundle("bad magic"));
    }
    if bundle[4] != BUNDLE_VERSION {
        return Err(CryptoError::MalformedBundle("unsupported bundle version"));
    }
    if bundle[5] != AEAD_ID_XCHACHA20POLY1305 {
        return Err(CryptoError::MalformedBundle("unsupported aead id"));
    }
    let header = &bundle[0..HEADER_LEN];
    let nonce = XNonce::from_slice(&bundle[HEADER_LEN..HEADER_LEN + NONCE_LEN]);
    let ciphertext = &bundle[HEADER_LEN + NONCE_LEN..];
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose_secret())
        .expect("DomainKey is exactly 32 bytes");
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map_err(|_| CryptoError::Aead)
}

fn encrypt_inner(
    key: &DomainKey,
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let header = build_header();
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose_secret())
        .expect("DomainKey is exactly 32 bytes");
    let xnonce = XNonce::from_slice(nonce);
    let ciphertext = cipher
        .encrypt(
            xnonce,
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| CryptoError::Aead)?;
    let mut bundle = Vec::with_capacity(HEADER_LEN + NONCE_LEN + ciphertext.len());
    bundle.extend_from_slice(&header);
    bundle.extend_from_slice(nonce);
    bundle.extend_from_slice(&ciphertext);
    Ok(bundle)
}

#[cfg(test)]
pub(crate) fn encrypt_with_nonce(
    key: &DomainKey,
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    encrypt_inner(key, nonce, plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_domain_key, derive_master_key, read_or_init_salt, Domain, MasterKey};
    use tempfile::TempDir;

    fn fixed_master() -> MasterKey {
        MasterKey::from_bytes_for_test([0x42u8; 32])
    }

    fn key_for(domain: Domain) -> DomainKey {
        derive_domain_key(&fixed_master(), domain)
    }

    #[test]
    fn roundtrip_random() {
        let key = key_for(Domain::Conversation);
        let plaintext = b"the operator returns voluntarily";
        let bundle = encrypt(&key, plaintext).unwrap();
        let decoded = decrypt(&key, &bundle).unwrap();
        assert_eq!(decoded.as_slice(), plaintext);
    }

    // Pinned KAT for the bundle layout. Locks magic, version, aead id,
    // nonce placement, header-as-AAD binding, and the upstream
    // chacha20poly1305 crate's output. Any drift breaks loudly.
    // Updating the expected bytes orphans every existing ciphertext.
    #[test]
    fn kat_pinned_layout() {
        let master = MasterKey::from_bytes_for_test([0x33u8; 32]);
        let key = derive_domain_key(&master, Domain::Conversation);
        let nonce = [0x77u8; NONCE_LEN];
        let plaintext = b"cardinal";
        let bundle = encrypt_with_nonce(&key, &nonce, plaintext).unwrap();
        let expected: Vec<u8> = vec![
            // header: magic | version | aead id
            b'C', b'O', b'O', b'E', 0x01, 0x02,
            // nonce (24 × 0x77)
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            // ciphertext (8 bytes) || Poly1305 tag (16 bytes) = 24 bytes
            0x19, 0xd2, 0x68, 0x3b, 0xe4, 0xbb, 0xef, 0x49, 0x4e, 0xab, 0x73, 0x8f, 0xb6, 0x68,
            0x4e, 0xde, 0xa8, 0x7a, 0xa6, 0xf1, 0x59, 0xf3, 0xff, 0x35,
        ];
        assert_eq!(bundle, expected);
    }

    #[test]
    fn wrong_key_fails_auth() {
        let k1 = key_for(Domain::Rapport);
        let k2 = key_for(Domain::FriendshipFloor);
        let bundle = encrypt(&k1, b"secret").unwrap();
        match decrypt(&k2, &bundle) {
            Err(CryptoError::Aead) => {}
            other => panic!("expected Aead error, got {:?}", other),
        }
    }

    #[test]
    fn tampered_ciphertext_fails_auth() {
        let key = key_for(Domain::Conversation);
        let mut bundle = encrypt(&key, b"some plaintext content").unwrap();
        // Flip the first ciphertext byte (just past the nonce, before the tag).
        bundle[HEADER_LEN + NONCE_LEN] ^= 0x01;
        match decrypt(&key, &bundle) {
            Err(CryptoError::Aead) => {}
            other => panic!("expected Aead error, got {:?}", other),
        }
    }

    #[test]
    fn tampered_tag_fails_auth() {
        let key = key_for(Domain::Conversation);
        let mut bundle = encrypt(&key, b"some plaintext content").unwrap();
        // Flip the last byte (in the trailing 16-byte Poly1305 tag).
        let last = bundle.len() - 1;
        bundle[last] ^= 0x01;
        match decrypt(&key, &bundle) {
            Err(CryptoError::Aead) => {}
            other => panic!("expected Aead error, got {:?}", other),
        }
    }

    // Proves the AAD binding is real on the decrypt side. We can't test
    // it by tampering header bytes — every header byte in v1 is
    // framing-validated, so any flip yields MalformedBundle before the
    // AEAD runs. Instead, we construct a bundle whose ciphertext was
    // produced *without* binding the header as AAD, then confirm
    // decrypt rejects it. If AAD weren't bound on decrypt, this would
    // succeed; because it is bound, the auth check fails.
    #[test]
    fn header_aad_is_bound() {
        let key = key_for(Domain::Conversation);
        let nonce = [0x55u8; NONCE_LEN];
        let plaintext = b"payload";

        let cipher = XChaCha20Poly1305::new_from_slice(key.expose_secret()).unwrap();
        let xnonce = XNonce::from_slice(&nonce);
        // Encrypt with empty AAD (default Payload), not the header.
        let ciphertext_no_aad = cipher.encrypt(xnonce, plaintext.as_ref()).unwrap();

        let mut bundle = Vec::new();
        bundle.extend_from_slice(&BUNDLE_MAGIC);
        bundle.push(BUNDLE_VERSION);
        bundle.push(AEAD_ID_XCHACHA20POLY1305);
        bundle.extend_from_slice(&nonce);
        bundle.extend_from_slice(&ciphertext_no_aad);

        match decrypt(&key, &bundle) {
            Err(CryptoError::Aead) => {}
            other => panic!("expected Aead error from missing-AAD bundle, got {:?}", other),
        }
    }

    #[test]
    fn rejects_too_short() {
        let key = key_for(Domain::Conversation);
        let too_short = [0u8; MIN_BUNDLE_LEN - 1];
        match decrypt(&key, &too_short) {
            Err(CryptoError::MalformedBundle("too short")) => {}
            other => panic!("expected too-short rejection, got {:?}", other),
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let key = key_for(Domain::Conversation);
        let mut buf = vec![0u8; MIN_BUNDLE_LEN];
        buf[0..4].copy_from_slice(b"XXXX");
        match decrypt(&key, &buf) {
            Err(CryptoError::MalformedBundle("bad magic")) => {}
            other => panic!("expected bad-magic rejection, got {:?}", other),
        }
    }

    #[test]
    fn rejects_unknown_bundle_version() {
        let key = key_for(Domain::Conversation);
        let mut buf = vec![0u8; MIN_BUNDLE_LEN];
        buf[0..4].copy_from_slice(&BUNDLE_MAGIC);
        buf[4] = 0xFF;
        buf[5] = AEAD_ID_XCHACHA20POLY1305;
        match decrypt(&key, &buf) {
            Err(CryptoError::MalformedBundle("unsupported bundle version")) => {}
            other => panic!("expected version rejection, got {:?}", other),
        }
    }

    #[test]
    fn rejects_unknown_aead_id() {
        let key = key_for(Domain::Conversation);
        let mut buf = vec![0u8; MIN_BUNDLE_LEN];
        buf[0..4].copy_from_slice(&BUNDLE_MAGIC);
        buf[4] = BUNDLE_VERSION;
        buf[5] = 0xFF;
        match decrypt(&key, &buf) {
            Err(CryptoError::MalformedBundle("unsupported aead id")) => {}
            other => panic!("expected aead-id rejection, got {:?}", other),
        }
    }

    #[test]
    fn nonce_uniqueness_probe() {
        let key = key_for(Domain::Conversation);
        let plaintext = b"same plaintext";
        let b1 = encrypt(&key, plaintext).unwrap();
        let b2 = encrypt(&key, plaintext).unwrap();
        assert_ne!(b1, b2);
        let n1 = &b1[HEADER_LEN..HEADER_LEN + NONCE_LEN];
        let n2 = &b2[HEADER_LEN..HEADER_LEN + NONCE_LEN];
        assert_ne!(n1, n2);
    }

    #[test]
    fn empty_plaintext_roundtrips() {
        let key = key_for(Domain::Conversation);
        let bundle = encrypt(&key, &[]).unwrap();
        // Smallest valid bundle: header + nonce + 16-byte tag (no body).
        assert_eq!(bundle.len(), MIN_BUNDLE_LEN);
        let decoded = decrypt(&key, &bundle).unwrap();
        assert!(decoded.is_empty());
    }

    // End-to-end: passphrase → master via Argon2id → per-domain key via
    // HKDF → encrypt → decrypt. Pays the Argon2id cost once, like
    // kdf::tests::kat_pinned_parameters already does.
    #[test]
    fn end_to_end_passphrase_to_plaintext() {
        let dir = TempDir::new().unwrap();
        let salt_path = dir.path().join("salt");
        let salt = read_or_init_salt(&salt_path).unwrap();
        let master = derive_master_key(b"operator-passphrase", &salt).unwrap();
        let key = derive_domain_key(&master, Domain::OperatorKnowledge);
        let plaintext = b"he deflects when the consulting offer comes up";
        let bundle = encrypt(&key, plaintext).unwrap();
        let decoded = decrypt(&key, &bundle).unwrap();
        assert_eq!(decoded.as_slice(), plaintext);
    }
}
