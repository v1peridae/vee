use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

pub enum HashAlg {Sha256, Sha512, Sha1}
pub struct Integrity {pub alg: HashAlg, pub digest: Vec<u8>}

impl Integrity {
    pub fn parse(sri: &str) -> Result<Self> {
        let (alg_str, b64) = sri
            .split_once('-')
            .ok_or_else(|| anyhow::anyhow!("invalid SRI format:{}", sri))?;

        let alg = match alg_str {
            "sha256" => HashAlg::Sha256,
            "sha512" => HashAlg::Sha512,
            "sha1" => HashAlg::Sha1,
            other => bail!("unsupported hash algorithm:{}", other),
        };

        let digest = STANDARD.decode(b64)?;
        Ok(Self { alg, digest })
    }

    pub fn cache_key(&self) -> String {
        let alg = match self.alg {
            HashAlg::Sha256 => "sha256",
            HashAlg::Sha512 => "sha512",
            HashAlg::Sha1 => "sha1",
        };
        format!("{}-{}", alg, hex::encode(&self.digest))
    }
}

enum HashState {Sha1(Sha1), Sha256(Sha256), Sha512(Sha512)}
pub struct IntegrityVerifier {state: HashState, expected: Vec<u8>}

impl Integrity {
    pub fn verifier(&self) -> IntegrityVerifier {
        let state = match self.alg {
            HashAlg::Sha1 => HashState::Sha1(Sha1::new()),
            HashAlg::Sha256 => HashState::Sha256(Sha256::new()),
            HashAlg::Sha512 => HashState::Sha512(Sha512::new()),
        };
        IntegrityVerifier {
            state,
            expected: self.digest.clone(),
        }
    }
}

impl IntegrityVerifier {
    pub fn update(&mut self, data: &[u8]) {
        match &mut self.state {
            HashState::Sha1(hasher) => hasher.update(data),
            HashState::Sha256(hasher) => hasher.update(data),
            HashState::Sha512(hasher) => hasher.update(data),
        }
    }

    pub fn verify(self) -> bool {
        let computed = match self.state {
            HashState::Sha1(hasher) => hasher.finalize().to_vec(),
            HashState::Sha256(hasher) => hasher.finalize().to_vec(),
            HashState::Sha512(hasher) => hasher.finalize().to_vec(),
        };
        computed == self.expected
    }
}
