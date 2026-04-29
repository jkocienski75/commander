// Vault — first-run setup and per-launch unlock for the operator's
// encrypted state. Sits atop the crypto substrate (kdf + derive + envelope)
// shipped in §2; consumed by §3's UI and the startup gating in lib.rs::run.
//
// On first run, setup_passphrase derives a MasterKey from the operator's
// passphrase via Argon2id (writing the salt file alongside) and writes a
// sentinel ciphertext at <coo_dir>/sentinel — the AEAD bundle of a fixed
// known plaintext under a passphrase-derived lock key. On subsequent
// launches, unlock re-derives the same master key from the entered
// passphrase and verifies it by attempting to decrypt the sentinel.
//
// Tag-mismatch from an AEAD-uniform standpoint means "wrong passphrase
// or tampered sentinel" — both surface to the caller as
// UnlockError::WrongPassphrase, since the passphrase-not-stored rule
// (RAPPORT-STATE-MODEL.md §6.2) means we cannot distinguish them without
// additional persistent state.

use crate::crypto::{
    derive_domain_key, derive_lock_key, derive_master_key, encrypt, read_or_init_salt,
    CryptoError, Domain, DomainKey, MasterKey,
};
use std::fs;
use std::path::{Path, PathBuf};

const SALT_FILENAME: &str = "salt";
const SENTINEL_FILENAME: &str = "sentinel";

// 16-byte known plaintext for sentinel verification. The "-01" suffix is
// the rotation hedge: a future v0.x could change the constant deliberately
// without ambiguity. Locked by sentinel_kat_pinned_layout below — drift
// orphans every existing install's sentinel.
const SENTINEL_PLAINTEXT: &[u8; 16] = b"COO-LOCK-SENT-01";

pub struct UnlockedVault {
    #[allow(dead_code)] // read by domain_key; §3 (b) consumer
    master: MasterKey,
}

impl UnlockedVault {
    // The only consumption path. Mirrors the MasterKey::expose_secret
    // discipline — vault holds the master, never exposes it; callers ask
    // for a domain key and get one bound to the passphrase that unlocked
    // the vault.
    #[allow(dead_code)] // §3 (b) consumer
    pub fn domain_key(&self, domain: Domain) -> DomainKey {
        derive_domain_key(&self.master, domain)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InitState {
    Uninitialized,
    Initialized,
    Inconsistent(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("could not resolve home directory")]
    NoHomeDir,
    #[error("vault is already initialized")]
    AlreadyInitialized,
    #[error("vault state is inconsistent: {0}")]
    Inconsistent(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum UnlockError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("vault is not initialized")]
    Uninitialized,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("vault state is inconsistent: {0}")]
    Inconsistent(&'static str),
}

fn salt_path(coo_dir: &Path) -> PathBuf {
    coo_dir.join(SALT_FILENAME)
}

fn sentinel_path(coo_dir: &Path) -> PathBuf {
    coo_dir.join(SENTINEL_FILENAME)
}

#[allow(dead_code)] // §3 (d) startup gating will consume this
pub fn default_coo_dir() -> Result<PathBuf, VaultError> {
    let home = dirs::home_dir().ok_or(VaultError::NoHomeDir)?;
    Ok(home.join(".coo"))
}

#[allow(dead_code)] // §3 (c)/(d) will consume this
pub fn inspect(coo_dir: &Path) -> InitState {
    let salt_present = salt_path(coo_dir).exists();
    let sentinel_present = sentinel_path(coo_dir).exists();
    match (salt_present, sentinel_present) {
        (false, false) => InitState::Uninitialized,
        (true, true) => InitState::Initialized,
        (true, false) => InitState::Inconsistent("missing sentinel"),
        (false, true) => InitState::Inconsistent("missing salt"),
    }
}

#[allow(dead_code)] // §3 (c) will consume this
pub fn setup_passphrase(coo_dir: &Path, passphrase: &[u8]) -> Result<UnlockedVault, VaultError> {
    match inspect(coo_dir) {
        InitState::Uninitialized => {}
        InitState::Initialized => return Err(VaultError::AlreadyInitialized),
        InitState::Inconsistent(reason) => return Err(VaultError::Inconsistent(reason)),
    }
    fs::create_dir_all(coo_dir)?;
    let salt = read_or_init_salt(&salt_path(coo_dir))?;
    let master = derive_master_key(passphrase, &salt)?;
    let lock_key = derive_lock_key(&master);
    let bundle = encrypt(&lock_key, SENTINEL_PLAINTEXT)?;
    fs::write(sentinel_path(coo_dir), &bundle)?;
    Ok(UnlockedVault { master })
}

#[allow(dead_code)] // §3 (c)/(d) will consume this
pub fn unlock(coo_dir: &Path, passphrase: &[u8]) -> Result<UnlockedVault, UnlockError> {
    match inspect(coo_dir) {
        InitState::Uninitialized => return Err(UnlockError::Uninitialized),
        InitState::Inconsistent(reason) => return Err(UnlockError::Inconsistent(reason)),
        InitState::Initialized => {}
    }
    let salt = read_or_init_salt(&salt_path(coo_dir))?;
    let master = derive_master_key(passphrase, &salt)?;
    let lock_key = derive_lock_key(&master);
    let bundle = fs::read(sentinel_path(coo_dir))?;
    match crate::crypto::decrypt(&lock_key, &bundle) {
        Ok(plaintext) => {
            // The envelope authenticated. Verify the plaintext matches the
            // sentinel constant. A different plaintext that authenticates
            // would mean someone produced a valid bundle under our derived
            // lock key with different plaintext — vanishingly unlikely
            // given AEAD authentication, but if it ever happens, treat it
            // as inconsistent state rather than success.
            if plaintext.as_slice() != SENTINEL_PLAINTEXT {
                return Err(UnlockError::Inconsistent("sentinel plaintext mismatch"));
            }
            Ok(UnlockedVault { master })
        }
        Err(CryptoError::Aead) => Err(UnlockError::WrongPassphrase),
        Err(CryptoError::MalformedBundle(reason)) => Err(UnlockError::Inconsistent(reason)),
        Err(other) => Err(UnlockError::Crypto(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn inspect_uninitialized() {
        let dir = TempDir::new().unwrap();
        assert_eq!(inspect(dir.path()), InitState::Uninitialized);
    }

    #[test]
    fn inspect_initialized() {
        let dir = TempDir::new().unwrap();
        let _ = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        assert_eq!(inspect(dir.path()), InitState::Initialized);
    }

    #[test]
    fn inspect_inconsistent_salt_only() {
        let dir = TempDir::new().unwrap();
        fs::write(salt_path(dir.path()), [0u8; 34]).unwrap();
        assert_eq!(
            inspect(dir.path()),
            InitState::Inconsistent("missing sentinel")
        );
    }

    #[test]
    fn inspect_inconsistent_sentinel_only() {
        let dir = TempDir::new().unwrap();
        fs::write(sentinel_path(dir.path()), [0u8; 62]).unwrap();
        assert_eq!(
            inspect(dir.path()),
            InitState::Inconsistent("missing salt")
        );
    }

    #[test]
    fn setup_then_unlock_roundtrip() {
        let dir = TempDir::new().unwrap();
        let v1 = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let v2 = unlock(dir.path(), b"operator-passphrase").unwrap();
        // Same master ⇒ same domain keys for the same Domain.
        assert_eq!(
            v1.domain_key(Domain::Rapport).expose_secret(),
            v2.domain_key(Domain::Rapport).expose_secret(),
        );
    }

    #[test]
    fn unlock_wrong_passphrase() {
        let dir = TempDir::new().unwrap();
        let _ = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        match unlock(dir.path(), b"wrong-passphrase") {
            Err(UnlockError::WrongPassphrase) => {}
            Err(other) => panic!("expected WrongPassphrase, got {:?}", other),
            Ok(_) => panic!("expected WrongPassphrase, got Ok"),
        }
    }

    #[test]
    fn unlock_uninitialized() {
        let dir = TempDir::new().unwrap();
        match unlock(dir.path(), b"anything") {
            Err(UnlockError::Uninitialized) => {}
            Err(other) => panic!("expected Uninitialized, got {:?}", other),
            Ok(_) => panic!("expected Uninitialized, got Ok"),
        }
    }

    #[test]
    fn setup_when_already_initialized() {
        let dir = TempDir::new().unwrap();
        let _ = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        match setup_passphrase(dir.path(), b"any-other-passphrase") {
            Err(VaultError::AlreadyInitialized) => {}
            Err(other) => panic!("expected AlreadyInitialized, got {:?}", other),
            Ok(_) => panic!("expected AlreadyInitialized, got Ok"),
        }
    }

    // Pinned KAT for the sentinel bundle. Locks the lock-key info string
    // (via derive_lock_key), the sentinel plaintext (SENTINEL_PLAINTEXT),
    // and the envelope crate's output for a fixed master + fixed nonce
    // simultaneously. Drift in any of the three breaks the KAT loudly.
    // Updating the expected bytes orphans every existing install's
    // sentinel.
    #[test]
    fn sentinel_kat_pinned_layout() {
        let master = MasterKey::from_bytes_for_test([0x33u8; 32]);
        let lock_key = derive_lock_key(&master);
        let nonce = [0x77u8; 24];
        let bundle =
            crate::crypto::encrypt_with_nonce(&lock_key, &nonce, SENTINEL_PLAINTEXT).unwrap();
        let expected: Vec<u8> = vec![
            // header: magic | version | aead id
            b'C', b'O', b'O', b'E', 0x01, 0x02,
            // nonce (24 × 0x77)
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            // ciphertext (16 bytes) || Poly1305 tag (16 bytes)
            0xd5, 0x10, 0x26, 0xb9, 0x16, 0x91, 0xe2, 0x86, 0xf6, 0x08, 0x8b, 0xf4, 0x3c, 0xd7,
            0x05, 0xe2, 0x76, 0xd9, 0x04, 0xc4, 0x8e, 0xb0, 0xb3, 0x18, 0xf1, 0x6b, 0x93, 0xb6,
            0xfa, 0x22, 0xc9, 0x3d,
        ];
        assert_eq!(bundle, expected);
    }
}
