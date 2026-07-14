//! Git config / identity binding internal module tree.
//!
//! Split out of `commands::git` (which historically mixed IPC thin-shell,
//! business logic, pure-string splice helpers, and filesystem wrappers in
//! a single 1850-line file). See
//! `docs/superpowers/plans/2026-07-13-split-commands-git.md` for the
//! rationale and the function-to-module mapping.

pub mod bind;
pub mod init;
pub mod io;
pub mod ops;
pub mod protocol;
pub mod splice;
