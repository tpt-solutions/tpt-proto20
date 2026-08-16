//! Schema history manifest support (spec §20.5).
//!
//! A manifest records the evolution of a schema package: versions, fingerprints,
//! compatibility policies, migration notes, reserved IDs, and deprecation dates.
//! It is serializable for storage in a schema registry.

use serde::{Deserialize, Serialize};

/// A single recorded schema version entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    /// Version label, e.g. `"user.v1"`.
    pub version: String,
    /// Fingerprint of the compiled descriptor at this version.
    pub fingerprint: String,
    /// Compatibility policy in force at this version.
    pub policy: String,
}

/// A reserved-id or reserved-name record with rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservedRecord {
    /// Symbol the reservation applies to.
    pub symbol: String,
    /// Reserved numeric ids / id ranges.
    pub ids: Vec<ReservedId>,
    /// Reserved names.
    pub names: Vec<String>,
}

/// A reserved id entry: single id or inclusive range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservedId {
    /// A single reserved id.
    Single(u32),
    /// An inclusive range `lo..=hi`.
    Range(u32, u32),
}

/// A deprecation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deprecation {
    /// Symbol being deprecated.
    pub symbol: String,
    /// ISO-8601 deprecation date.
    pub date: String,
}

/// A schema history manifest (spec §20.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SchemaHistoryManifest {
    /// Package name this manifest describes.
    pub package: Option<String>,
    /// Recorded schema versions.
    pub versions: Vec<VersionEntry>,
    /// Compatibility policies in force over time.
    pub policies: Vec<String>,
    /// Migration notes, keyed by version transition.
    pub migration_notes: Vec<String>,
    /// Reserved ids per symbol.
    pub reserved: Vec<ReservedRecord>,
    /// Deprecation dates.
    pub deprecations: Vec<Deprecation>,
}

impl SchemaHistoryManifest {
    /// Creates an empty manifest for the given package.
    pub fn new(package: Option<String>) -> SchemaHistoryManifest {
        SchemaHistoryManifest {
            package,
            ..Default::default()
        }
    }

    /// Records a version entry.
    pub fn record_version(
        &mut self,
        version: impl Into<String>,
        fingerprint: impl Into<String>,
        policy: impl Into<String>,
    ) {
        self.versions.push(VersionEntry {
            version: version.into(),
            fingerprint: fingerprint.into(),
            policy: policy.into(),
        });
    }

    /// Records a compatibility policy.
    pub fn record_policy(&mut self, policy: impl Into<String>) {
        self.policies.push(policy.into());
    }

    /// Records a migration note.
    pub fn record_migration_note(&mut self, note: impl Into<String>) {
        self.migration_notes.push(note.into());
    }

    /// Records a reserved-id / reserved-name entry.
    pub fn record_reserved(
        &mut self,
        symbol: impl Into<String>,
        ids: Vec<ReservedId>,
        names: Vec<String>,
    ) {
        self.reserved.push(ReservedRecord {
            symbol: symbol.into(),
            ids,
            names,
        });
    }

    /// Records a deprecation date.
    pub fn record_deprecation(&mut self, symbol: impl Into<String>, date: impl Into<String>) {
        self.deprecations.push(Deprecation {
            symbol: symbol.into(),
            date: date.into(),
        });
    }

    /// Serializes the manifest to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a manifest from JSON.
    pub fn from_json(json: &str) -> Result<SchemaHistoryManifest, serde_json::Error> {
        serde_json::from_str(json)
    }
}
