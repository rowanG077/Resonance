//! Decode authored UV motion for title lighting.
pub(crate) mod field;
mod title;
pub(crate) use title::{cook as cook_title, profiles as title_profiles};
