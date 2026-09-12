use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
};
use solana_transaction_status::EncodedTransactionWithStatusMeta;
use tokio::time::sleep;

use core::fmt;
use std::{
    collections::HashMap,
    mem,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use serde::de::{self, Visitor};
use serde::ser::Serializer;
use serde::Deserializer;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DASBlock {
    pub block_metadata: BlockMetadata,
    pub das_transactions: Vec<EncodedTransactionWithStatusMeta>,
    pub das_accounts: HashMap<Pubkey, (Option<Account>, u64)>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockMetadata {
    pub slot: Slot,
    // In Solana, slots can be skipped. So there are not necessarily sequential.
    pub parent_slot: Slot,
    pub block_time: UnixTimestamp,
    pub blockhash: Hash,
    pub parent_blockhash: Hash,
    pub block_height: u64,
}

// Maximum length of a 32 byte base58 encoded hash
const MAX_BASE58_LEN: usize = 44;

/// `Hash` is a struct that represents a 32-byte hash.
///
/// This is similar to a Solana Public Key, which is also a 32-byte hash. However, while a Solana
/// Public Key is specifically an ed25519 public key, a `Hash` is not necessarily an ed25519 hash.
///
/// Also a `Hash` is used to represent other types of hashes, such as account hashes and Merkle
/// tree node hashes. This is why `Hash` is a separate struct from the the Solana Public Key struct.
#[derive(Default, Clone, PartialEq, Eq, Hash)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn to_base58(&self) -> String {
        bs58::encode(self.0).into_string()
    }

    pub fn new_unique() -> Self {
        // Slightly hacky way to get 32 random bytes
        Hash(Pubkey::new_unique().to_bytes())
    }
}

#[derive(Error, Debug, Serialize, Clone, PartialEq, Eq)]
pub enum ParseHashError {
    #[error("String is the wrong size")]
    WrongSize,
    #[error("Invalid hash input")]
    Invalid,
}

impl TryFrom<&str> for Hash {
    type Error = ParseHashError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.len() > MAX_BASE58_LEN {
            return Err(ParseHashError::WrongSize);
        }
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParseHashError::Invalid)?;

        bytes.try_into()
    }
}

impl From<Hash> for Vec<u8> {
    fn from(val: Hash) -> Self {
        val.0.to_vec()
    }
}

impl From<[u8; 32]> for Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Hash(bytes)
    }
}

impl From<Hash> for [u8; 32] {
    fn from(val: Hash) -> Self {
        val.0
    }
}

impl TryFrom<Vec<u8>> for Hash {
    type Error = ParseHashError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.len() != mem::size_of::<Hash>() {
            Err(ParseHashError::WrongSize)
        } else {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| ParseHashError::Invalid)?;
            Ok(Hash(bytes))
        }
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl From<Hash> for String {
    fn from(val: Hash) -> Self {
        val.to_base58()
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.to_base58())
    }
}

struct Base58VisitorHash;

impl<'de> Visitor<'de> for Base58VisitorHash {
    type Value = Hash;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a base58 encoded string")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Hash::try_from(value).map_err(|e| E::custom(e.to_string()))
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(Base58VisitorHash)
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let base58_string = bs58::encode(self.0).into_string();
        serializer.serialize_str(&base58_string)
    }
}

#[test]
fn test_serialization() {
    // Hacky way to get 32 bytes
    let hash = Hash(Pubkey::new_unique().to_bytes());
    let serialized = serde_json::to_string(&hash).unwrap();
    let deserialized: Hash = serde_json::from_str(&serialized).unwrap();
    assert_eq!(hash, deserialized);
}

pub struct RateLimiter {
    rate: u32,
    per: Duration,
    allowance: f64,
    last_check: Instant,
}

impl RateLimiter {
    pub fn new(rate: u32, per: Duration) -> Self {
        RateLimiter {
            rate,
            per,
            allowance: rate as f64,
            last_check: Instant::now(),
        }
    }

    pub async fn acquire(&mut self) {
        let now = Instant::now();
        let time_passed = now - self.last_check;
        self.last_check = now;

        self.allowance += time_passed.as_secs_f64() * (self.rate as f64 / self.per.as_secs_f64());
        if self.allowance > self.rate as f64 {
            self.allowance = self.rate as f64;
        }

        if self.allowance < 1.0 {
            let wait_time = Duration::from_secs_f64(
                (1.0 - self.allowance) * (self.per.as_secs_f64() / self.rate as f64),
            );
            sleep(wait_time).await;
            self.allowance = 1.0;
        } else {
            self.allowance -= 1.0;
        }
    }
}
