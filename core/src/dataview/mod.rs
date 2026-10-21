//! Dataview, built in: the page index, the query language and its functions.
//! Queries written for the Dataview plugin (`dataview` blocks, `= …` inline
//! queries) run here unchanged; `dataviewjs` goes through `crate::script`.

pub mod eval;
pub mod index;
pub mod parse;
pub mod query;
pub mod value;

pub use index::Index;
pub use value::{Settings, Value};
