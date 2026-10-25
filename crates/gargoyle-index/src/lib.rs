//! the code indexm what every file defines and uses, read with tree-sitter,
//! cached by gits id for each file so later runs only parse what changed
//!
//! a file without a grammar isn't indexed, its review gets no related code

mod languages;

pub use languages::{Grammar, grammar_of};
