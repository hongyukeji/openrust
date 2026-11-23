use std::iter;

use super::{ArrayKey, CollectionError, LazyCollection, index_values};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn zip<U: 'source>(
        self,
        items: impl Into<LazyCollection<'source, U>>,
    ) -> LazyCollection<'source, (Option<T>, Option<U>)> {
        let items = items.into();
        LazyCollection::from_iterators(move || {
            let mut left = self.getIterator();
            let mut right = items.getIterator();
            index_values(iter::from_fn(move || {
                let left = match left.next().transpose() {
                    Ok(entry) => entry,
                    Err(error) => return Some(Err(error)),
                };
                match (left, right.next().transpose()) {
                    (_, Err(error)) => Some(Err(error)),
                    (None, Ok(None)) => None,
                    (left, Ok(right)) => Some(Ok((
                        left.map(|(_, value)| value),
                        right.map(|(_, value)| value),
                    ))),
                }
            }))
        })
    }

    pub fn zipMany<U: 'source, I: Into<LazyCollection<'source, U>>>(
        self,
        columns: impl IntoIterator<Item = I>,
    ) -> LazyCollection<'source, (Option<T>, Vec<Option<U>>)> {
        let columns: Vec<_> = columns.into_iter().map(Into::into).collect();
        LazyCollection::from_iterators(move || {
            let mut first = self.getIterator();
            let mut columns: Vec<_> = columns.iter().map(LazyCollection::getIterator).collect();
            index_values(iter::from_fn(move || {
                let first = match first.next().transpose() {
                    Ok(entry) => entry.map(|(_, item)| item),
                    Err(error) => return Some(Err(error)),
                };
                let rest: Result<Vec<_>, _> = columns
                    .iter_mut()
                    .map(|column| {
                        column
                            .next()
                            .transpose()
                            .map(|entry| entry.map(|(_, item)| item))
                    })
                    .collect();
                match rest {
                    Ok(rest) if first.is_some() || rest.iter().any(Option::is_some) => {
                        Some(Ok((first, rest)))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            }))
        })
    }

    pub fn combine<U: 'source>(
        self,
        values: impl Into<LazyCollection<'source, U>>,
    ) -> LazyCollection<'source, U>
    where
        T: Into<ArrayKey>,
    {
        let values = values.into();
        LazyCollection::from_iterators(move || {
            let mut keys = self.getIterator();
            let mut values = values.getIterator();
            iter::from_fn(move || {
                let key = match keys.next().transpose() {
                    Ok(entry) => entry,
                    Err(error) => return Some(Err(error)),
                };
                match (key, values.next().transpose()) {
                    (_, Err(error)) => Some(Err(error)),
                    (Some((_, key)), Ok(Some((_, value)))) => {
                        Some(Ok((key.into().normalized(), value)))
                    }
                    (None, Ok(None)) => None,
                    _ => Some(Err(CollectionError::InvalidArgument)),
                }
            })
        })
    }
}
