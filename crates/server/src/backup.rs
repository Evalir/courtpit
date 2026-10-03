//! Database backups to S3-compatible object storage.
//!
//! The storage side lives behind [`ObjectStore`] so the backup job can run against an
//! in-memory fake in tests; [`s3::S3Store`] is the production implementation.

pub mod s3;

use std::{fmt::Debug, pin::Pin};

use tokio::io::AsyncRead;

/// A stream of bytes to upload. A source that fails midway (for example `pg_dump` exiting
/// non-zero) must report it as a read error at the end, so the upload is aborted rather than
/// completed.
pub type DumpReader = Box<dyn AsyncRead + Send + Unpin>;

/// Boxed future returned by [`ObjectStore`] methods (keeps the trait object-safe).
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// Object storage (S3-compatible).
pub trait ObjectStore: Send + Sync + Debug {
    /// Streams `body` to `key` without knowing its length; returns the bytes stored. On any
    /// failure nothing is left behind (a partial multipart upload is aborted).
    fn put_stream<'a>(&'a self, key: &'a str, body: DumpReader) -> StoreFuture<'a, u64>;
    /// All keys starting with `prefix`.
    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>>;
    /// Deletes one object.
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()>;
}

/// `text` cut to at most `max` characters, with an ellipsis when it was longer.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_characters() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "abc…");
        assert_eq!(truncate("éééé", 2), "éé…");
    }
}
