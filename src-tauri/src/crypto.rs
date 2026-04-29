// Per RAPPORT-STATE-MODEL.md §6 — encryption substrate.
// Submodules:
//   kdf       — Argon2id passphrase → MasterKey, salt file persistence
//   derive    — HKDF-SHA256 master → per-domain key
//   envelope  — XChaCha20-Poly1305 AEAD encrypt/decrypt over a DomainKey

mod derive;
mod envelope;
mod kdf;

#[allow(unused_imports)] // §3 will consume these
pub use derive::{derive_domain_key, derive_lock_key, Domain, DomainKey};
#[allow(unused_imports)] // §3 will consume these
pub use envelope::{decrypt, encrypt};
#[allow(unused_imports)] // §3 will consume these
pub use kdf::{derive_master_key, read_or_init_salt, MasterKey, Salt};

// Test-only re-export of the fixed-nonce encrypt helper. Lets sibling
// modules (vault::tests) pin KAT bundle bytes without paying a random
// nonce. Mirrors the #[cfg(test)] visibility of envelope::encrypt_with_nonce.
#[cfg(test)]
pub(crate) use envelope::encrypt_with_nonce;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("argon2 error: {0}")]
    Argon2(#[from] argon2::Error),
    #[error("salt file is malformed: {0}")]
    MalformedSaltFile(&'static str),
    #[error("ciphertext bundle is malformed: {0}")]
    MalformedBundle(&'static str),
    #[error("aead authentication failed")]
    Aead,
    #[error("random source error: {0}")]
    Random(#[from] getrandom::Error),
}
