use super::{ArrayKey, Collection, CollectionError, CollectionResult};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<T> Collection<T> {
    pub fn forPage(self, page: usize, per_page: usize) -> Self {
        let offset = page.saturating_sub(1).saturating_mul(per_page);
        Self::fromEntries(self.into_iter().skip(offset).take(per_page))
    }

    pub fn nth(self, step: usize, offset: usize) -> CollectionResult<Self> {
        if step == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        Ok(Self::make(
            self.items.into_values().skip(offset).step_by(step),
        ))
    }

    pub fn split(self, groups: usize) -> CollectionResult<Collection<Self>> {
        if groups == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        let len = self.count();
        let mut items = self.into_iter();
        Ok(Collection::make((0..groups.min(len)).map(|index| {
            let size = len / groups + usize::from(index < len % groups);
            reindex(items.by_ref().take(size))
        })))
    }

    pub fn splitIn(self, groups: usize) -> CollectionResult<Collection<Self>> {
        if groups == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        let size = self.count().div_ceil(groups);
        Ok(self.chunk(size))
    }

    pub fn chunkWhile(
        self,
        mut predicate: impl FnMut(&T, &ArrayKey, &Self) -> bool,
    ) -> Collection<Self> {
        let mut chunks = Vec::new();
        let mut current = Self::new();
        for (key, item) in self {
            if !current.isEmpty() && !predicate(&item, &key, &current) {
                chunks.push(std::mem::take(&mut current));
            }
            current.put(key, item);
        }
        if !current.isEmpty() {
            chunks.push(current);
        }
        Collection::make(chunks)
    }

    pub fn chunkBy<K: PartialEq>(
        self,
        mut project: impl FnMut(&T, &ArrayKey) -> K,
    ) -> Collection<Self> {
        self.chunkWhile(|item, key, chunk| {
            let (last_key, last) = chunk.items.last().expect("current chunk is nonempty");
            project(item, key) == project(last, last_key)
        })
    }

    pub fn prepend(&mut self, value: T, key: Option<ArrayKey>) -> &mut Self {
        let items = std::mem::take(self);
        *self = if let Some(key) = key.map(ArrayKey::normalized) {
            let remaining = items.into_iter().filter(|(existing, _)| existing != &key);
            // Preserve the old key position rules of the other entries.
            let mut result = Self::new();
            result.put(key.clone(), value);
            for (key, item) in remaining {
                result.put(key, item);
            }
            result
        } else {
            reindex(std::iter::once((ArrayKey::Integer(0), value)).chain(items))
        };
        self
    }

    pub fn unshift(&mut self, items: impl IntoIterator<Item = T>) -> &mut Self {
        let mut items = items.into_iter().peekable();
        if items.peek().is_some() {
            let previous = std::mem::take(self);
            *self = reindex(
                items
                    .map(|item| (ArrayKey::Integer(0), item))
                    .chain(previous),
            );
        }
        self
    }

    pub fn shift(&mut self) -> Option<T> {
        let mut items = std::mem::take(self).into_iter();
        let removed = items.next().map(|(_, value)| value);
        *self = reindex(items);
        removed
    }

    pub fn shiftMany(&mut self, count: usize) -> Self {
        if count == 0 {
            return Self::new();
        }
        let mut items = std::mem::take(self).into_iter();
        let removed = Self::make(items.by_ref().take(count).map(|(_, item)| item));
        *self = reindex(items);
        removed
    }

    pub fn splice(
        &mut self,
        offset: isize,
        length: Option<isize>,
        replacement: impl IntoIterator<Item = T>,
    ) -> Self {
        let len = self.count();
        let start = if offset < 0 {
            len.saturating_sub(offset.unsigned_abs())
        } else {
            offset.unsigned_abs().min(len)
        };
        let room = len - start;
        let count = match length {
            Some(length) if length < 0 => room.saturating_sub(length.unsigned_abs()),
            Some(length) => room.min(length.unsigned_abs()),
            None => room,
        };
        let mut items = std::mem::take(self).into_iter();
        let head: Vec<_> = items.by_ref().take(start).collect();
        let removed = reindex(items.by_ref().take(count));
        let replacements = replacement
            .into_iter()
            .map(|item| (ArrayKey::Integer(0), item));
        *self = reindex(head.into_iter().chain(replacements).chain(items));
        removed
    }

    pub fn multiply(self, count: usize) -> Self
    where
        T: Clone,
    {
        Self::make((0..count).flat_map(|_| self.items.values().cloned()))
    }
}

fn reindex<T>(items: impl IntoIterator<Item = (ArrayKey, T)>) -> Collection<T> {
    let mut next = 0_i64;
    Collection::fromEntries(items.into_iter().map(|(key, item)| {
        let key = if matches!(key, ArrayKey::Integer(_)) {
            let current = next;
            next += 1;
            ArrayKey::Integer(current)
        } else {
            key
        };
        (key, item)
    }))
}
