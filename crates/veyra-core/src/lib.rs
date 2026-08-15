pub mod action;
pub mod catalog;
pub mod config;
pub mod search;

pub use action::{Action, ActionKind};
pub use catalog::{CatalogItem, IconRef, ItemCategory, seed_catalog};
pub use search::{SearchIndex, SearchResult, search};
