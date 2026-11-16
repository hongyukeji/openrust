use std::{collections::HashSet, hash::Hash, rc::Rc};

use super::{ArrayKey, CollectionError, CollectionResult, Entry, LazyCollection};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn unique(self) -> Self
    where
        T: Eq + Hash + Clone,
    {
        self.uniqueBy(|value, _| value.clone())
    }

    pub fn uniqueStrict(self) -> Self
    where
        T: Eq + Hash + Clone,
    {
        self.unique()
    }

    pub fn uniqueBy<K: Eq + Hash + 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> K + 'source,
    ) -> Self {
        let project = Rc::new(project);
        Self::from_iterators(move || {
            let project = Rc::clone(&project);
            let mut seen = HashSet::new();
            self.getIterator().filter(move |entry| match entry {
                Ok((key, value)) => seen.insert(project(value, key)),
                Err(_) => true,
            })
        })
    }

    pub fn isEmpty(&self) -> CollectionResult<bool> {
        Ok(self.first()?.is_none())
    }

    pub fn isNotEmpty(&self) -> CollectionResult<bool> {
        Ok(!self.isEmpty()?)
    }

    pub fn contains(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.containsWhere(|item, _| item == value)
    }

    pub fn containsStrict(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.contains(value)
    }

    pub fn containsWhere(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        Ok(self.firstWhere(predicate)?.is_some())
    }

    pub fn doesntContain(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        Ok(!self.contains(value)?)
    }

    pub fn doesntContainStrict(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.doesntContain(value)
    }

    pub fn some(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<bool> {
        self.containsWhere(predicate)
    }

    pub fn firstOrFail(&self) -> CollectionResult<T> {
        self.first()?.ok_or(CollectionError::ItemNotFound)
    }

    pub fn firstOrFailWhere(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<T> {
        self.firstWhere(predicate)?
            .ok_or(CollectionError::ItemNotFound)
    }

    pub fn sole(&self) -> CollectionResult<T> {
        self.soleWhere(|_, _| true)
    }

    pub fn soleWhere(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<T> {
        let mut matches = self.matching(predicate);
        let (key, value) = matches
            .next()
            .transpose()?
            .ok_or(CollectionError::ItemNotFound)?;
        match matches.next().transpose()? {
            None => Ok(value),
            Some((next_key, next_value)) if key == next_key => Ok(next_value),
            Some(_) => Err(CollectionError::MultipleItemsFound),
        }
    }

    pub fn hasSole(&self) -> CollectionResult<bool> {
        self.hasSoleWhere(|_, _| true)
    }

    pub fn hasSoleWhere(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        let mut matches = self.matching(predicate);
        Ok(matches.next().transpose()?.is_some() && matches.next().transpose()?.is_none())
    }

    pub fn hasMany(&self) -> CollectionResult<bool> {
        self.hasManyWhere(|_, _| true)
    }

    pub fn hasManyWhere(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        let mut matches = self.matching(predicate);
        Ok(matches.next().transpose()?.is_some() && matches.next().transpose()?.is_some())
    }

    fn matching(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> impl Iterator<Item = CollectionResult<Entry<T>>> {
        self.getIterator().filter(move |entry| match entry {
            Ok((key, value)) => predicate(value, key),
            Err(_) => true,
        })
    }
}
