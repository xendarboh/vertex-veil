//! Predicate parity fixtures.
//!
//! The runtime-side `match_predicate` and the Phase 2 Noir circuit are two
//! implementations of one logical function. A parity fixture is a JSON file
//! carrying a public `(requester, provider, round)` tuple plus the expected
//! predicate outcome. Both implementations run the same fixture; divergence
//! fails the parity suite.
//!
//! The fixture JSON contains only public fields. Private witness values are
//! never embedded; when a fixture needs to illustrate a "private constraint
//! would fail" case, it uses a synthetic capability / round / identity
//! mismatch in public fields instead. That keeps the parity suite safe to
//! fail loudly without leaking anything.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::predicate::PredicateDenial;
use crate::shared_types::{PublicIntent, RoundId};

/// Expected outcome of running the predicate against a fixture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExpectedOutcome {
    Accept,
    Reject { code: String },
}

impl ExpectedOutcome {
    /// Convert a runtime-side predicate result into the canonical expected
    /// form.
    pub fn from_runtime(result: Result<(), PredicateDenial>) -> Self {
        match result {
            Ok(()) => ExpectedOutcome::Accept,
            Err(denial) => ExpectedOutcome::Reject {
                code: denial.tag().to_string(),
            },
        }
    }
}

/// One predicate parity fixture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParityFixture {
    pub name: String,
    pub round: RoundId,
    pub requester: PublicIntent,
    pub provider: PublicIntent,
    pub expected: ExpectedOutcome,
}

impl ParityFixture {
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn load_dir(dir: impl AsRef<Path>) -> std::io::Result<Vec<Self>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                out.push(Self::load(entry.path())?);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}
