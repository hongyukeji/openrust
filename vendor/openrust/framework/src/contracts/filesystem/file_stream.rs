use crate::filesystem::{FilesystemFuture, FilesystemResult};

/// Produces a file in chunks without loading the full file into memory.
pub trait FileStream: Send {
    fn next_chunk(&mut self) -> FilesystemFuture<'_, FilesystemResult<Option<Vec<u8>>>>;
}
