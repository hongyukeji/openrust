use std::{iter, rc::Rc};

use serde_json::Value;

use super::{ArrayKey, Collection, LazyCollection, index_values};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn collapse<U: 'source>(self) -> LazyCollection<'source, U>
    where
        T: Into<LazyCollection<'source, U>>,
    {
        self.collapseWithKeys().values()
    }

    pub fn collapseWithKeys<U: 'source>(self) -> LazyCollection<'source, U>
    where
        T: Into<LazyCollection<'source, U>>,
    {
        LazyCollection::from_iterators(move || {
            let mut outer = self.getIterator();
            let mut inner = None;
            iter::from_fn(move || {
                loop {
                    if let Some(entry) = inner.as_mut().and_then(Iterator::next) {
                        return Some(entry);
                    }
                    match outer.next()? {
                        Ok((_, group)) => inner = Some(group.into().getIterator()),
                        Err(error) => return Some(Err(error)),
                    }
                }
            })
        })
    }

    pub fn flatMap<U: 'source, G: Into<LazyCollection<'source, U>> + 'source>(
        self,
        callback: impl Fn(&T, &ArrayKey) -> G + 'source,
    ) -> LazyCollection<'source, U> {
        self.map(callback).collapse()
    }
}

impl LazyCollection<'_, Value> {
    pub fn flatten(self, depth: Option<usize>) -> Self {
        let depth = depth.filter(|depth| *depth != 0).unwrap_or(usize::MAX);
        Self::from_iterators(move || {
            let mut source = self.getIterator();
            let mut stack: Vec<(usize, JsonValues)> = Vec::new();
            index_values(iter::from_fn(move || {
                loop {
                    let (value, remaining) = if let Some((remaining, children)) = stack.last_mut() {
                        let Some(value) = children.next() else {
                            stack.pop();
                            continue;
                        };
                        (value, *remaining)
                    } else {
                        match source.next()? {
                            Ok((_, value)) => (value, depth),
                            Err(error) => return Some(Err(error)),
                        }
                    };
                    if remaining == 0 {
                        return Some(Ok(value));
                    }
                    match value {
                        Value::Array(items) => {
                            stack.push((remaining - 1, Box::new(items.into_iter())));
                        }
                        Value::Object(items) => {
                            stack.push((remaining - 1, Box::new(items.into_values())));
                        }
                        value => return Some(Ok(value)),
                    }
                }
            }))
        })
    }
}

impl<'source, T: Clone + 'source> From<Collection<T>> for LazyCollection<'source, T> {
    fn from(collection: Collection<T>) -> Self {
        collection.lazy()
    }
}

impl<'source, T: Clone + 'source> From<Vec<T>> for LazyCollection<'source, T> {
    fn from(values: Vec<T>) -> Self {
        let values = Rc::new(values);
        Self::from_iterators(move || {
            let values = Rc::clone(&values);
            index_values((0..values.len()).map(move |index| Ok(values[index].clone())))
        })
    }
}

impl<'source, T: Clone + 'source, const N: usize> From<[T; N]> for LazyCollection<'source, T> {
    fn from(values: [T; N]) -> Self {
        Vec::from(values).into()
    }
}

type JsonValues = Box<dyn Iterator<Item = Value>>;
