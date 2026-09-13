//! Everything the configuration contract needs after a document exists.
//!
//! A service's configuration surface is published as one JSON document attached to its image —
//! `spec/v1/` is what that document is. Producing one needs the service's own types and can only
//! be per-language. **Everything downstream of it needs a document and nothing else**, and that is
//! this crate.
//!
//! ```text
//! per language, once                    this crate, for everyone
//! â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€       â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//! types â”€â”€> Schema â”€â”€> Contract â”€â”€JSONâ”€â”€> render   the tables, the file, the labels
//!                                         stamp    build identity onto a document
//!                                         conform  the refusals, and a tier
//!                                         validate the published meta-schema
//!                                         image    labels and Dockerfile block, read back
//! ```
//!
//! So a new implementation's obligation is one sentence: emit a document that conforms. Not a
//! renderer per format, not a validator, not a test kit. That is what makes "and any future
//! language" a claim rather than a hope.
//!
//! # It does not depend on any implementation
//!
//! Not on `terrace-config`, not on anything that links a loader. A renderer that reached for one
//! producer's types would be that producer's renderer wearing a shared name, and the Java side
//! would be right to treat it that way. The dependency is asserted in CI, because it is the one
//! property here that nothing else would notice breaking.
//!
//! # Reading a document written by somebody else
//!
//! [`document`] reads tolerantly and gates on the envelope; every closed vocabulary carries an
//! `other` arm so that one unknown enum value in one key does not fail a document this build can
//! otherwise read. What it will not do is guess: `producer.loader` names the library whose
//! environment reads a document's `text_constraint` patterns were measured against, and a consumer
//! meeting a loader it does not know must skip the read that depends on them rather than perform
//! it with the wrong rules.

pub mod classify;
pub mod conform;
pub mod diff;
pub mod document;
mod error;
pub mod render;
pub mod report;
pub mod text;
pub mod union;
pub mod validate;
pub mod value;

#[cfg(feature = "k8s")]
pub mod gate;
#[cfg(feature = "helm")]
pub mod helm;
#[cfg(feature = "k8s")]
pub mod k8s;

pub use classify::{Classification, Kind, classify};
pub use conform::{Tier, Violation};
pub use diff::{Change, ContractDiff, Severity};
pub use document::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Dialect, External, ExternalVar, Key, LoaderRole,
    LoaderVar, Producer, SCHEMA_VERSION, Schema, TextForm, Unknown, Unreachable,
};
pub use error::Error;
pub use render::{Column, Format, Options};
pub use report::{Finding, Level, Report};
pub use union::{Merged, Union, union_contracts};
pub use value::{Range, Reads, reads_for};
