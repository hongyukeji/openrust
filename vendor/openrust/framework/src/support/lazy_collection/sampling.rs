use super::{Collection, CollectionError, CollectionResult, LazyCollection};
use crate::support::arr::random_index;

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn crossJoin(self, others: &'source [&'source [T]]) -> LazyCollection<'source, Vec<T>>
    where
        T: Clone,
    {
        self.passthru(move |items| items.crossJoin(others))
    }

    pub fn randItem(&self) -> CollectionResult<T> {
        let items = Self::collect(self)?;
        if items.isEmpty() {
            return Err(CollectionError::ItemNotFound);
        }
        let index = random_index(items.count())?;
        Ok(items
            .into_iter()
            .nth(index)
            .expect("random index in bounds")
            .1)
    }

    pub fn random(&self, count: usize, preserve_keys: bool) -> CollectionResult<Self>
    where
        T: Clone,
    {
        Self::collect(self)?
            .random(count, preserve_keys)
            .map(Into::into)
    }

    pub fn shuffle(self) -> Self {
        self.try_passthru(Collection::shuffle)
    }
}
