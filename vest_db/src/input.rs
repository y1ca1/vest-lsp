//! Source file input for the Salsa database.

/// Input representing a source file's content and metadata.
#[salsa::input]
pub struct SourceFile {
    /// The identity of this file (as URI string).
    #[returns(ref)]
    pub uri: String,
    /// The version number (from LSP).
    pub version: i32,
    /// The source text content.
    #[returns(ref)]
    pub text: String,
}
