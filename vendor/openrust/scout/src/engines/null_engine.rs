use super::{Engine, SearchResults};
use crate::{Builder, ScoutFuture, Searchable};
#[derive(Default)]
pub struct NullEngine;
impl<M: Searchable> Engine<M> for NullEngine {
    fn update<'a>(&'a self, _: &'a [M]) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn delete<'a>(&'a self, _: &'a [M]) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn search<'a>(&'a self, _: &'a Builder<M>) -> ScoutFuture<'a, SearchResults> {
        Box::pin(async { Ok(SearchResults::default()) })
    }
    fn paginate<'a>(&'a self, b: &'a Builder<M>, _: u64, _: u64) -> ScoutFuture<'a, SearchResults> {
        self.search(b)
    }
    fn flush<'a>(&'a self, _: &'a M) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn create_index<'a>(&'a self, _: &'a str, _: &'a serde_json::Value) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn delete_index<'a>(&'a self, _: &'a str) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
