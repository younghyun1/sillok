//! Static classification of shell commands; nothing is executed or expanded.

mod bounds;
mod context;
mod programs;
pub mod scan;
mod tables;
#[cfg(test)]
mod tests;
mod tokenize;
mod writers;
