//! Sillok: a structured chronicle of agent work.
//!
//! The library target exists for the `sillok` binary, integration tests, and
//! examples. Only the command-line interface and the v3 event format are
//! covered by semantic versioning; these modules may change in any release.

#[doc(hidden)]
pub mod app;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod commands;
#[doc(hidden)]
pub mod context;
#[doc(hidden)]
pub mod domain;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod legacy;
#[doc(hidden)]
pub mod storage;
#[doc(hidden)]
pub mod sync;
