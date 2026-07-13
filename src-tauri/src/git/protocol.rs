//! Reserved for protocol-classification helpers.
//!
//! Today, URL classification lives in `crate::git_config::classify_remote_url`
//! and is consumed directly by callers. A future task may move URL-derivation
//! logic (e.g. `deriveSshUrlFromHttps` parity with the TypeScript side) into
//! this module. For now it is a placeholder so the four-module skeleton
//! compiles.

#[allow(dead_code)]
pub(crate) fn _placeholder() {}
