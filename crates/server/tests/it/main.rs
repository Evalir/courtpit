//! Integration tests. One binary (`it`) with a module per area keeps link times down.
#![expect(clippy::unwrap_used, reason = "tests fail loudly")]

mod common;
mod health;
