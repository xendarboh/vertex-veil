//! Runtime-configurable capability tags.
//!
//! `INTENT.md` declares capability tags as coarse, runtime-configurable
//! labels with `GPU`, `CPU`, `LLM`, and `ZK_DEV` as illustrative examples for
//! the first delivery context. The allowed label set is therefore carried in
//! [`TopologyConfig`] rather than being baked into the type system.
//!
//! Every parsed [`CapabilityTag`] must:
//!
//! - be non-empty
//! - match `[A-Z][A-Z0-9_]*`
//! - appear in the configured [`CapabilityTagSet`]
//!
//! These checks run at config load time so invalid or unknown tags fail
//! before runtime starts.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// A coarse capability tag, e.g. `GPU`, `CPU`, `LLM`, `ZK_DEV`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityTag(String);

impl CapabilityTag {
    /// Validate the label shape. Does not check membership in a tag set.
    pub fn parse_shape(raw: &str) -> Result<Self, ConfigError> {
        if raw.is_empty() {
            return Err(ConfigError::MalformedCapabilityTag("empty".into()));
        }
        let mut chars = raw.chars();
        let first = chars.next().unwrap();
        if !first.is_ascii_uppercase() {
            return Err(ConfigError::MalformedCapabilityTag(format!(
                "must start with uppercase letter: {raw}"
            )));
        }
        for c in chars {
            if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                return Err(ConfigError::MalformedCapabilityTag(format!(
                    "contains invalid character {c:?} in {raw}"
                )));
            }
        }
        Ok(CapabilityTag(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The configured universe of capability tags for a run.
///
/// The set is runtime-configurable and may contain a single tag (for a
/// minimal demo) or the illustrative defaults (`GPU`, `CPU`, `LLM`,
/// `ZK_DEV`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityTagSet {
    tags: BTreeSet<CapabilityTag>,
}

impl CapabilityTagSet {
    /// Build a tag set, enforcing shape rules and non-emptiness.
    pub fn new<I, S>(tags: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = BTreeSet::new();
        for raw in tags {
            let tag = CapabilityTag::parse_shape(raw.as_ref())?;
            out.insert(tag);
        }
        if out.is_empty() {
            return Err(ConfigError::InvalidTopology(
                "capability tag set must not be empty".into(),
            ));
        }
        Ok(CapabilityTagSet { tags: out })
    }

    /// Illustrative defaults used by the compute-task matching demo.
    pub fn illustrative_defaults() -> Self {
        CapabilityTagSet::new(["GPU", "CPU", "LLM", "ZK_DEV"]).expect("valid defaults")
    }

    /// Normalize a raw tag against this set. Unknown tags are rejected.
    pub fn normalize(&self, raw: &str) -> Result<CapabilityTag, ConfigError> {
        let tag = CapabilityTag::parse_shape(raw)?;
        if self.tags.contains(&tag) {
            Ok(tag)
        } else {
            Err(ConfigError::UnknownCapabilityTag(tag.as_str().to_string()))
        }
    }

    pub fn contains(&self, tag: &CapabilityTag) -> bool {
        self.tags.contains(tag)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CapabilityTag> {
        self.tags.iter()
    }

    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_contain_illustrative_tags() {
        let set = CapabilityTagSet::illustrative_defaults();
        for tag in ["GPU", "CPU", "LLM", "ZK_DEV"] {
            assert!(set.normalize(tag).is_ok(), "{tag} should be valid");
        }
    }

    #[test]
    fn rejects_lowercase_shape() {
        assert!(CapabilityTag::parse_shape("gpu").is_err());
    }

    #[test]
    fn rejects_leading_digit() {
        assert!(CapabilityTag::parse_shape("1GPU").is_err());
    }

    #[test]
    fn rejects_unknown_tag_against_set() {
        let set = CapabilityTagSet::new(["GPU"]).unwrap();
        assert!(matches!(
            set.normalize("CPU"),
            Err(ConfigError::UnknownCapabilityTag(_))
        ));
    }

    #[test]
    fn empty_set_rejected() {
        let empty: [&str; 0] = [];
        assert!(CapabilityTagSet::new(empty).is_err());
    }

    #[test]
    fn single_tag_accepted() {
        let set = CapabilityTagSet::new(["GPU"]).unwrap();
        assert_eq!(set.len(), 1);
    }
}
