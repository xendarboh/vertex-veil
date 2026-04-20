//! Error types for config parsing and artifact construction.
//!
//! Error variants never carry private witness values. Config parsing errors
//! name the offending field but not the value when the field is declared
//! private.

use thiserror::Error;

/// Errors returned while loading or validating a [`TopologyConfig`].
///
/// [`TopologyConfig`]: crate::config::TopologyConfig
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("topology config is invalid: {0}")]
    InvalidTopology(String),

    #[error("unknown capability tag: {0}")]
    UnknownCapabilityTag(String),

    #[error("capability tag is malformed: {0}")]
    MalformedCapabilityTag(String),

    #[error("missing required field: {0}")]
    MissingField(&'static str),

    #[error("invalid stable public key: {0}")]
    InvalidStableKey(String),

    #[error("provider list must not be empty")]
    EmptyProviderList,

    #[error("duplicate node identifier: {0}")]
    DuplicateNodeId(String),

    #[error("unsafe config path: {0}")]
    UnsafePath(String),

    #[error("failed to read config file: {0}")]
    Io(String),

    #[error("failed to parse TOML: {0}")]
    Parse(String),
}

impl ConfigError {
    /// Construct an [`Io`] error without exposing absolute paths or private
    /// payload bytes.
    pub(crate) fn io<E: std::fmt::Display>(err: E) -> Self {
        ConfigError::Io(err.to_string())
    }

    /// Construct a [`Parse`] error from the underlying TOML deserializer.
    ///
    /// TOML parse errors include an echo of the offending source line in their
    /// `Display` form. That echo is a leak risk for config files that carry
    /// would-be private values (e.g. a fixture budget mistyped as a non-field
    /// assignment). We keep the location header ("TOML parse error at line N,
    /// column M") and the trailing diagnostic, but strip the source-echo lines
    /// and the caret marker before surfacing the error.
    pub(crate) fn parse<E: std::fmt::Display>(err: E) -> Self {
        let raw = err.to_string();
        let redacted: String = raw
            .lines()
            .filter(|line| {
                // TOML formats its source-echo section using pipe characters
                // (e.g. `  |`, `4 | <content>`, `  | ^`). Every pipe-bearing
                // line belongs to that echo and may reveal raw input content.
                // The keepable lines — the "TOML parse error at line N,
                // column M" header and the trailing diagnostic text — do not
                // contain pipes, so excluding pipe lines is a safe cut.
                !line.contains('|')
            })
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        ConfigError::Parse(redacted)
    }
}

/// Errors returned while writing or manipulating public coordination
/// artifacts.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArtifactError {
    #[error("artifact output path is invalid: {0}")]
    InvalidOutputPath(String),

    #[error("artifact directory already contains a run: {0}")]
    DirectoryNotEmpty(String),

    #[error("duplicate commitment entry for node in round {round}")]
    DuplicateCommitment { round: u64 },

    #[error("io error writing artifact: {0}")]
    Io(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

impl ArtifactError {
    pub(crate) fn io<E: std::fmt::Display>(err: E) -> Self {
        ArtifactError::Io(err.to_string())
    }

    pub(crate) fn serialization<E: std::fmt::Display>(err: E) -> Self {
        ArtifactError::Serialization(err.to_string())
    }
}
