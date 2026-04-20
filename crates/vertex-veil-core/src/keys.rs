//! Stable node identifiers.
//!
//! A [`NodeId`] is a 32-byte public key rendered as lowercase hexadecimal in
//! all public artifacts. The byte-lexicographic order of the raw bytes defines
//! the canonical "stable public key order" referenced in `INTENT.md`; it is
//! used to pick deterministic winners and the proposer rotation in later
//! phases.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ConfigError;

/// Stable public key bytes used as a node identifier.
///
/// The string form is lowercase hex with no prefix. Parsing rejects any other
/// representation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    /// Build a [`NodeId`] from the raw 32 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        NodeId(bytes)
    }

    /// Expose the raw bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Encode as lowercase hex without a prefix.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Debug output shows a short prefix only, so logs stay compact.
        let hex = self.to_hex();
        write!(f, "NodeId({}…)", &hex[..8.min(hex.len())])
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FromStr for NodeId {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ConfigError::InvalidStableKey("empty".into()));
        }
        if s.len() != 64 {
            return Err(ConfigError::InvalidStableKey(format!(
                "expected 64 hex chars, got {}",
                s.len()
            )));
        }
        if !s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(ConfigError::InvalidStableKey(
                "must be lowercase hex with no prefix".into(),
            ));
        }
        let mut out = [0u8; 32];
        hex::decode_to_slice(s, &mut out)
            .map_err(|e| ConfigError::InvalidStableKey(e.to_string()))?;
        Ok(NodeId(out))
    }
}

impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        NodeId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_lowercase_hex() {
        let s = "a".repeat(64);
        let id = NodeId::from_str(&s).unwrap();
        assert_eq!(id.to_hex(), s);
    }

    #[test]
    fn rejects_uppercase() {
        assert!(NodeId::from_str(&"A".repeat(64)).is_err());
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(NodeId::from_str("abcd").is_err());
    }

    #[test]
    fn debug_is_redacted_prefix() {
        let s = "a".repeat(64);
        let id = NodeId::from_str(&s).unwrap();
        let debug = format!("{:?}", id);
        assert!(debug.starts_with("NodeId("));
        assert!(!debug.contains(&s));
    }

    #[test]
    fn byte_order_is_lex() {
        let low = NodeId::from_bytes([0u8; 32]);
        let high = NodeId::from_bytes([0xffu8; 32]);
        assert!(low < high);
    }
}
