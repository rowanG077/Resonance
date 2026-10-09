//! Decode authored UV motion for title lighting.
pub(crate) mod field;
mod title;
pub(crate) use title::{bind as bind_title, cook as cook_title};
