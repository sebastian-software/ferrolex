//! Umbrella crate for Ferrolex.
//!
//! Applications should usually depend on this crate. Lower-level crates remain
//! available for integrations that only need tokenization, dictionary handling,
//! source extraction, or Lexios-powered brand validation.

pub use ferrolex_brand as brand;
pub use ferrolex_core as core;
pub use ferrolex_dict as dict;
pub use ferrolex_hunspell as hunspell;
pub use ferrolex_json as json;
pub use ferrolex_oxc as oxc;
pub use ferrolex_tokenizer as tokenizer;
