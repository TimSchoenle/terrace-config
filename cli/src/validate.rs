//! A document against the published meta-schema.
//!
//! The first question a new implementation asks, and the one `spec/v1/contract.schema.json` exists
//! to answer. It is embedded rather than read from disk, because the binary that answers it runs
//! inside a build container with no checkout of this repository — and a validator that silently
//! found no schema would report success without checking anything.

use crate::Error;

/// The meta-schema this build validates against, as published.
pub const META_SCHEMA: &str = include_str!("../../spec/v1/contract.schema.json");

/// Every way a document fails the meta-schema, most useful first.
///
/// Empty means it is well-formed. Well-formed is not the same as conforming: see
/// [`conform`](crate::conform), which holds a document to the rules a schema cannot express.
///
/// # Errors
/// [`Error::Invalid`] when the input is not JSON, or when the embedded meta-schema does not
/// compile — the second being a defect in this build rather than in the document.
pub fn validate(document: &str) -> Result<Vec<String>, Error> {
    let instance: serde_json::Value =
        serde_json::from_str(document).map_err(|e| Error::Invalid(format!("not JSON: {e}")))?;
    let meta: serde_json::Value = serde_json::from_str(META_SCHEMA)
        .map_err(|e| Error::Invalid(format!("the embedded meta-schema is not JSON: {e}")))?;

    let validator = jsonschema::validator_for(&meta).map_err(|e| {
        Error::Invalid(format!(
            "the embedded meta-schema does not compile, which is a defect in this build rather \
             than in the document: {e}"
        ))
    })?;

    Ok(validator
        .iter_errors(&instance)
        .map(|error| format!("{}: {error}", pointer(error.instance_path())))
        .collect())
}

/// Where in the document, as a pointer a reader can follow.
fn pointer(path: &jsonschema::paths::Location) -> String {
    let rendered = path.to_string();
    if rendered.is_empty() {
        "<document>".to_owned()
    } else {
        rendered
    }
}
