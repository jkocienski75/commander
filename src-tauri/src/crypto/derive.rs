use crate::crypto::MasterKey;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

const DOMAIN_KEY_BYTES: usize = 32;

// HKDF info strings per RAPPORT-STATE-MODEL.md §1 domain. These are the
// load-bearing identifiers that distinguish each domain's key from the
// others. Once any data is encrypted with a derived key, the corresponding
// info string becomes permanent — changing it orphans every existing
// ciphertext for that domain. The "v1" segment is the rotation hedge:
// a future v2 rotation would require explicit re-derivation under the
// operator's passphrase and a controlled re-encryption migration.
// Reachable from prod once §3 (b) wires the schema migration that calls
// UnlockedVault::domain_key. Until then, only test code touches them.
#[allow(dead_code)]
const INFO_RAPPORT: &[u8] = b"coo/v1/kdf/rapport";
#[allow(dead_code)]
const INFO_FRIENDSHIP_FLOOR: &[u8] = b"coo/v1/kdf/friendship-floor";
#[allow(dead_code)]
const INFO_OPERATOR_KNOWLEDGE: &[u8] = b"coo/v1/kdf/operator-knowledge";
#[allow(dead_code)]
const INFO_CONVERSATION: &[u8] = b"coo/v1/kdf/conversation";

// Lock-domain info string. Deliberately distinct from the four state-domain
// info strings above — the lock primitive is not a state domain per
// RAPPORT-STATE-MODEL.md §1, it is the substrate the vault layer uses to
// verify a passphrase without storing it. Same KAT discipline applies:
// renaming this string orphans every existing install's sentinel.
const INFO_LOCK: &[u8] = b"coo/v1/lock";

#[allow(dead_code)] // §3 (b) consumer
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Rapport,
    FriendshipFloor,
    OperatorKnowledge,
    Conversation,
}

impl Domain {
    #[allow(dead_code)] // §3 (b) consumer
    fn info(self) -> &'static [u8] {
        match self {
            Domain::Rapport => INFO_RAPPORT,
            Domain::FriendshipFloor => INFO_FRIENDSHIP_FLOOR,
            Domain::OperatorKnowledge => INFO_OPERATOR_KNOWLEDGE,
            Domain::Conversation => INFO_CONVERSATION,
        }
    }
}

pub struct DomainKey(Zeroizing<[u8; DOMAIN_KEY_BYTES]>);

impl DomainKey {
    pub fn expose_secret(&self) -> &[u8; DOMAIN_KEY_BYTES] {
        &self.0
    }
}

// HKDF-SHA256(master, info=domain.info(), L=32). Salt is None per
// RFC 5869 §3.1 — MasterKey is already a uniform-random 32-byte output
// of Argon2id, so no extra salt is needed. Infallible for our fixed
// 32-byte output: HKDF-SHA256's expand limit is 255 * 32 = 8160 bytes,
// and we are 256× under it.
#[allow(dead_code)] // §3 (b) consumer (via UnlockedVault::domain_key)
pub fn derive_domain_key(master: &MasterKey, domain: Domain) -> DomainKey {
    let hk = Hkdf::<Sha256>::new(None, master.expose_secret());
    let mut out: Zeroizing<[u8; DOMAIN_KEY_BYTES]> =
        Zeroizing::new([0u8; DOMAIN_KEY_BYTES]);
    hk.expand(domain.info(), &mut out[..])
        .expect("32-byte HKDF-SHA256 expand cannot exceed the 8160-byte OKM limit");
    DomainKey(out)
}

// Sibling primitive to derive_domain_key for the vault's passphrase
// sentinel. Same HKDF construction; different info string. Returns a
// DomainKey because the produced bytes are used identically — fed to
// crypto::envelope::encrypt as a 32-byte AEAD key — but the key is
// semantically the lock key, not a state-domain key.
pub fn derive_lock_key(master: &MasterKey) -> DomainKey {
    let hk = Hkdf::<Sha256>::new(None, master.expose_secret());
    let mut out: Zeroizing<[u8; DOMAIN_KEY_BYTES]> =
        Zeroizing::new([0u8; DOMAIN_KEY_BYTES]);
    hk.expand(INFO_LOCK, &mut out[..])
        .expect("32-byte HKDF-SHA256 expand cannot exceed the 8160-byte OKM limit");
    DomainKey(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_master() -> MasterKey {
        MasterKey::from_bytes_for_test([0x42u8; 32])
    }

    // RFC 5869 §A.1 Test Case 1. Validates the hkdf crate's HKDF-SHA256
    // implementation against the spec, independent of our domain code.
    // If this fails, the upstream crate changed semantics.
    #[test]
    fn rfc_5869_test_case_1() {
        let ikm = [0x0bu8; 22];
        let salt: [u8; 13] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        ];
        let info: [u8; 10] = [
            0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9,
        ];
        let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
        let mut okm = [0u8; 42];
        hk.expand(&info, &mut okm).unwrap();
        let expected: [u8; 42] = [
            0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36,
            0x2f, 0x2a, 0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56,
            0xec, 0xc4, 0xc5, 0xbf, 0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65,
        ];
        assert_eq!(okm, expected);
    }

    // Pinned KAT per domain. All four use the same fixed master
    // (32 bytes of 0x42); only the info string differs. If anyone
    // renames an info string, the corresponding KAT fails — that
    // failure is the signal that every existing ciphertext for the
    // affected domain has just been orphaned.
    #[test]
    fn kat_rapport() {
        let key = derive_domain_key(&fixed_master(), Domain::Rapport);
        let expected: [u8; 32] = [
            0x24, 0x10, 0x17, 0x44, 0xeb, 0xef, 0x96, 0x8b, 0x54, 0xb9, 0x54, 0xe1, 0x5d, 0x46,
            0xe1, 0xdb, 0xd3, 0x12, 0x79, 0x17, 0xc4, 0x06, 0x31, 0xbf, 0x5a, 0x2d, 0xac, 0x5f,
            0x5b, 0xe4, 0x92, 0x31,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    #[test]
    fn kat_friendship_floor() {
        let key = derive_domain_key(&fixed_master(), Domain::FriendshipFloor);
        let expected: [u8; 32] = [
            0x52, 0xce, 0xf1, 0x51, 0xe9, 0x55, 0x47, 0xb2, 0x1a, 0x0e, 0xf3, 0xf1, 0xb3, 0xd2,
            0x75, 0x8c, 0x02, 0x08, 0x39, 0x8c, 0xf3, 0x42, 0x55, 0x79, 0x2c, 0x4d, 0x5a, 0x85,
            0x76, 0xda, 0x04, 0x65,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    #[test]
    fn kat_operator_knowledge() {
        let key = derive_domain_key(&fixed_master(), Domain::OperatorKnowledge);
        let expected: [u8; 32] = [
            0x08, 0x90, 0x0e, 0x27, 0xa5, 0xc9, 0xac, 0xc9, 0x6b, 0xce, 0x87, 0x4d, 0xfe, 0xc9,
            0xc9, 0x02, 0x47, 0x45, 0x60, 0xbd, 0x03, 0x70, 0xa8, 0x0c, 0x52, 0x91, 0x1a, 0x2e,
            0x47, 0x64, 0x9d, 0xed,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    #[test]
    fn kat_conversation() {
        let key = derive_domain_key(&fixed_master(), Domain::Conversation);
        let expected: [u8; 32] = [
            0x83, 0xbc, 0xbc, 0xae, 0x36, 0x95, 0xf6, 0xd1, 0xe1, 0xad, 0x5c, 0x43, 0x3d, 0x3c,
            0xd7, 0xa7, 0x62, 0x71, 0x06, 0x52, 0xb1, 0xef, 0x7d, 0x76, 0x17, 0xdd, 0x64, 0xbe,
            0x5a, 0x02, 0xa8, 0x59,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    // KAT for the lock-key info string. Same fixed master as the four
    // domain KATs; only the info string differs. Renaming the lock info
    // string orphans every existing install's sentinel — that failure is
    // the signal here.
    #[test]
    fn kat_lock() {
        let key = derive_lock_key(&fixed_master());
        let expected: [u8; 32] = [
            0xd2, 0x77, 0xa0, 0x2a, 0x1e, 0xfa, 0xf2, 0x56, 0x5f, 0xed, 0x4b, 0x38, 0x07, 0x9b,
            0x0c, 0x3a, 0xcf, 0x7a, 0xe3, 0xa3, 0x71, 0xe7, 0xc6, 0x09, 0x7a, 0xba, 0x91, 0x48,
            0x9f, 0xec, 0xc8, 0xec,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    #[test]
    fn lock_key_distinct_from_domain_keys() {
        let m = fixed_master();
        let lock = derive_lock_key(&m);
        let r = derive_domain_key(&m, Domain::Rapport);
        let f = derive_domain_key(&m, Domain::FriendshipFloor);
        let o = derive_domain_key(&m, Domain::OperatorKnowledge);
        let c = derive_domain_key(&m, Domain::Conversation);
        for k in [
            r.expose_secret(),
            f.expose_secret(),
            o.expose_secret(),
            c.expose_secret(),
        ] {
            assert_ne!(lock.expose_secret(), k);
        }
    }

    #[test]
    fn distinct_domains_yield_distinct_keys() {
        let m = fixed_master();
        let r = derive_domain_key(&m, Domain::Rapport);
        let f = derive_domain_key(&m, Domain::FriendshipFloor);
        let o = derive_domain_key(&m, Domain::OperatorKnowledge);
        let c = derive_domain_key(&m, Domain::Conversation);
        let keys: [&[u8; 32]; 4] = [
            r.expose_secret(),
            f.expose_secret(),
            o.expose_secret(),
            c.expose_secret(),
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j], "domain keys {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn distinct_masters_yield_distinct_domain_keys() {
        let m1 = MasterKey::from_bytes_for_test([0x11u8; 32]);
        let m2 = MasterKey::from_bytes_for_test([0x22u8; 32]);
        let k1 = derive_domain_key(&m1, Domain::Rapport);
        let k2 = derive_domain_key(&m2, Domain::Rapport);
        assert_ne!(k1.expose_secret(), k2.expose_secret());
    }

    #[test]
    fn derivation_is_deterministic() {
        let m = fixed_master();
        let k1 = derive_domain_key(&m, Domain::Rapport);
        let k2 = derive_domain_key(&m, Domain::Rapport);
        assert_eq!(k1.expose_secret(), k2.expose_secret());
    }
}
