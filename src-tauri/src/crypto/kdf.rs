use crate::crypto::CryptoError;
use argon2::{Algorithm, Argon2, Params, Version};
use std::fs;
use std::path::Path;
use zeroize::Zeroizing;

// Argon2id parameters per RAPPORT-STATE-MODEL.md §6.2.
// RFC 9106's "second recommended option" with parallelism reduced to 1
// for portability; targets the §6.2 250–500ms band on desktop hardware.
// Persisted into the salt file (see SaltFile layout below) so future
// param changes do not orphan existing installs.
pub const ARGON2_M_COST: u32 = 65536; // 64 MiB
pub const ARGON2_T_COST: u32 = 3;
pub const ARGON2_P_COST: u32 = 1;

const MASTER_KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

// Salt file layout (34 bytes, little-endian):
//   [0..4)   magic = b"COO1"
//   [4]      file version = 0x01
//   [5]      kdf id = 0x01 (argon2id)
//   [6..10)  m_cost  (u32)
//   [10..14) t_cost  (u32)
//   [14..18) p_cost  (u32)
//   [18..34) salt bytes (16)
const SALT_FILE_MAGIC: [u8; 4] = *b"COO1";
const SALT_FILE_VERSION: u8 = 0x01;
const KDF_ID_ARGON2ID: u8 = 0x01;
const SALT_FILE_LEN: usize = 4 + 1 + 1 + 4 + 4 + 4 + SALT_BYTES; // = 34

pub struct MasterKey(Zeroizing<[u8; MASTER_KEY_BYTES]>);

impl MasterKey {
    pub fn expose_secret(&self) -> &[u8; MASTER_KEY_BYTES] {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Salt {
    bytes: [u8; SALT_BYTES],
    kdf_id: u8,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
}

impl Salt {
    fn fresh() -> Result<Self, CryptoError> {
        let mut bytes = [0u8; SALT_BYTES];
        getrandom::getrandom(&mut bytes)?;
        Ok(Self {
            bytes,
            kdf_id: KDF_ID_ARGON2ID,
            m_cost: ARGON2_M_COST,
            t_cost: ARGON2_T_COST,
            p_cost: ARGON2_P_COST,
        })
    }

    fn encode(&self) -> [u8; SALT_FILE_LEN] {
        let mut out = [0u8; SALT_FILE_LEN];
        out[0..4].copy_from_slice(&SALT_FILE_MAGIC);
        out[4] = SALT_FILE_VERSION;
        out[5] = self.kdf_id;
        out[6..10].copy_from_slice(&self.m_cost.to_le_bytes());
        out[10..14].copy_from_slice(&self.t_cost.to_le_bytes());
        out[14..18].copy_from_slice(&self.p_cost.to_le_bytes());
        out[18..34].copy_from_slice(&self.bytes);
        out
    }

    fn decode(raw: &[u8]) -> Result<Self, CryptoError> {
        if raw.len() != SALT_FILE_LEN {
            return Err(CryptoError::MalformedSaltFile("wrong length"));
        }
        if raw[0..4] != SALT_FILE_MAGIC {
            return Err(CryptoError::MalformedSaltFile("bad magic"));
        }
        if raw[4] != SALT_FILE_VERSION {
            return Err(CryptoError::MalformedSaltFile("unsupported file version"));
        }
        if raw[5] != KDF_ID_ARGON2ID {
            return Err(CryptoError::MalformedSaltFile("unsupported kdf id"));
        }
        let m_cost = u32::from_le_bytes(raw[6..10].try_into().unwrap());
        let t_cost = u32::from_le_bytes(raw[10..14].try_into().unwrap());
        let p_cost = u32::from_le_bytes(raw[14..18].try_into().unwrap());
        let mut bytes = [0u8; SALT_BYTES];
        bytes.copy_from_slice(&raw[18..34]);
        Ok(Self {
            bytes,
            kdf_id: raw[5],
            m_cost,
            t_cost,
            p_cost,
        })
    }
}

pub fn read_or_init_salt(path: &Path) -> Result<Salt, CryptoError> {
    if path.exists() {
        let raw = fs::read(path)?;
        Salt::decode(&raw)
    } else {
        let salt = Salt::fresh()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, salt.encode())?;
        Ok(salt)
    }
}

pub fn derive_master_key(passphrase: &[u8], salt: &Salt) -> Result<MasterKey, CryptoError> {
    let params = Params::new(
        salt.m_cost,
        salt.t_cost,
        salt.p_cost,
        Some(MASTER_KEY_BYTES),
    )?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out: Zeroizing<[u8; MASTER_KEY_BYTES]> = Zeroizing::new([0u8; MASTER_KEY_BYTES]);
    argon2.hash_password_into(passphrase, &salt.bytes, &mut out[..])?;
    Ok(MasterKey(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixed_salt() -> Salt {
        Salt {
            bytes: [0u8; SALT_BYTES],
            kdf_id: KDF_ID_ARGON2ID,
            m_cost: ARGON2_M_COST,
            t_cost: ARGON2_T_COST,
            p_cost: ARGON2_P_COST,
        }
    }

    #[test]
    fn derive_is_deterministic() {
        let salt = fixed_salt();
        let k1 = derive_master_key(b"passphrase", &salt).unwrap();
        let k2 = derive_master_key(b"passphrase", &salt).unwrap();
        assert_eq!(k1.expose_secret(), k2.expose_secret());
    }

    #[test]
    fn different_passphrases_yield_different_keys() {
        let salt = fixed_salt();
        let k1 = derive_master_key(b"passphrase-a", &salt).unwrap();
        let k2 = derive_master_key(b"passphrase-b", &salt).unwrap();
        assert_ne!(k1.expose_secret(), k2.expose_secret());
    }

    #[test]
    fn different_salts_yield_different_keys() {
        let s1 = Salt {
            bytes: [1u8; SALT_BYTES],
            ..fixed_salt()
        };
        let s2 = Salt {
            bytes: [2u8; SALT_BYTES],
            ..fixed_salt()
        };
        let k1 = derive_master_key(b"passphrase", &s1).unwrap();
        let k2 = derive_master_key(b"passphrase", &s2).unwrap();
        assert_ne!(k1.expose_secret(), k2.expose_secret());
    }

    // Known-answer test for our pinned production parameters.
    // Captured from a one-time run; locked here so any future change to
    // ARGON2_M_COST / T_COST / P_COST or to the underlying argon2 crate's
    // output breaks this test loudly. Updating the expected bytes is a
    // doctrine-level act — it orphans every existing operator install.
    #[test]
    fn kat_pinned_parameters() {
        let salt = fixed_salt();
        let key = derive_master_key(b"coo-test-vector", &salt).unwrap();
        let expected: [u8; MASTER_KEY_BYTES] = [
            0x1e, 0x20, 0x6e, 0x6f, 0x32, 0x45, 0x67, 0x2d, 0x63, 0xb9, 0x01, 0x24, 0xa0, 0x64,
            0xcf, 0xba, 0xda, 0x37, 0x8c, 0x39, 0x9d, 0x42, 0x85, 0x0d, 0x75, 0xf8, 0xfe, 0xb8,
            0x02, 0xec, 0xb0, 0x76,
        ];
        assert_eq!(key.expose_secret(), &expected);
    }

    #[test]
    fn salt_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("salt");
        let s1 = read_or_init_salt(&path).unwrap();
        let s2 = read_or_init_salt(&path).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn salt_file_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a").join("b").join("salt");
        let _ = read_or_init_salt(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn salt_file_rejects_wrong_length() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("salt");
        fs::write(&path, b"COO1").unwrap();
        match read_or_init_salt(&path) {
            Err(CryptoError::MalformedSaltFile("wrong length")) => {}
            other => panic!("expected wrong-length rejection, got {:?}", other),
        }
    }

    #[test]
    fn salt_file_rejects_bad_magic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("salt");
        fs::write(&path, [0u8; SALT_FILE_LEN]).unwrap();
        match read_or_init_salt(&path) {
            Err(CryptoError::MalformedSaltFile("bad magic")) => {}
            other => panic!("expected bad-magic rejection, got {:?}", other),
        }
    }

    #[test]
    fn salt_file_rejects_unknown_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("salt");
        let mut buf = [0u8; SALT_FILE_LEN];
        buf[0..4].copy_from_slice(&SALT_FILE_MAGIC);
        buf[4] = 0xFF;
        buf[5] = KDF_ID_ARGON2ID;
        fs::write(&path, buf).unwrap();
        match read_or_init_salt(&path) {
            Err(CryptoError::MalformedSaltFile("unsupported file version")) => {}
            other => panic!("expected version rejection, got {:?}", other),
        }
    }

    #[test]
    fn salt_file_rejects_unknown_kdf() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("salt");
        let mut buf = [0u8; SALT_FILE_LEN];
        buf[0..4].copy_from_slice(&SALT_FILE_MAGIC);
        buf[4] = SALT_FILE_VERSION;
        buf[5] = 0xFF;
        fs::write(&path, buf).unwrap();
        match read_or_init_salt(&path) {
            Err(CryptoError::MalformedSaltFile("unsupported kdf id")) => {}
            other => panic!("expected kdf rejection, got {:?}", other),
        }
    }
}
