//! What can go wrong reading or rendering a document.

/// A failure reading, rendering or checking a contract.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The document is not one this build can read, or does not say what it must.
    #[error("{0}")]
    Invalid(String),
    /// A file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// What was being read or written.
        path: String,
        /// Why it failed.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// An IO failure, with the path that caused it.
    ///
    /// `std::io::Error` alone says "the system cannot find the file specified" and not which file,
    /// which in a tool that reads one document per chart per image is the difference between a
    /// message a reader can act on and one they have to reproduce under a debugger.
    pub fn io(path: impl std::fmt::Display, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_string(),
            source,
        }
    }
}
