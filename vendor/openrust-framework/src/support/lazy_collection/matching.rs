use std::{collections::HashSet, hash::Hash, rc::Rc};

use super::{ArrayKey, CollectionError, CollectionResult, Entry, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn unique(self) -> Self
    where
        T: Eq + Hash + Clone,
    {
        self.unique_by(|value, _| value.clone())
    }

    pub fn unique_strict(self) -> Self
    where
        T: Eq + Hash + Clone,
    {
        self.unique()
    }

    pub fn unique_by<K: Eq + Hash + 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> K + 'source,
    ) -> Self {
        let project = Rc::new(project);
        Self::from_iterators(move || {
            let project = Rc::clone(&project);
            let mut seen = HashSet::new();
            self.get_iterator().filter(move |entry| match entry {
                Ok((key, value)) => seen.insert(project(value, key)),
                Err(_) => true,
            })
        })
    }

    pub fn is_empty(&self) -> CollectionResult<bool> {
        Ok(self.first()?.is_none())
    }

    pub fn is_not_empty(&self) -> CollectionResult<bool> {
        Ok(!self.is_empty()?)
    }

    pub fn contains(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.contains_where(|item, _| item == value)
    }

    pub fn contains_strict(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.contains(value)
    }

    pub fn contains_where(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        Ok(self.first_where(predicate)?.is_some())
    }

    pub fn doesnt_contain(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        Ok(!self.contains(value)?)
    }

    pub fn doesnt_contain_strict(&self, value: &T) -> CollectionResult<bool>
    where
        T: PartialEq,
    {
        self.doesnt_contain(value)
    }

    pub fn some(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<bool> {
        self.contains_where(predicate)
    }

    pub fn first_or_fail(&self) -> CollectionResult<T> {
        self.first()?.ok_or(CollectionError::ItemNotFound)
    }

    pub fn first_or_fail_where(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<T> {
        self.first_where(predicate)?
            .ok_or(CollectionError::ItemNotFound)
    }

    pub fn sole(&self) -> CollectionResult<T> {
        self.sole_where(|_, _| true)
    }

    pub fn sole_where(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<T> {
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

    pub fn has_sole(&self) -> CollectionResult<bool> {
        self.has_sole_where(|_, _| true)
    }

    pub fn has_sole_where(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        let mut matches = self.matching(predicate);
        Ok(matches.next().transpose()?.is_some() && matches.next().transpose()?.is_none())
    }

    /// Compatibility alias for `has_sole`.
    pub fn contains_one_item(&self) -> CollectionResult<bool> {
        self.has_sole()
    }

    /// Compatibility alias for `has_many`.
    pub fn contains_many_items(&self) -> CollectionResult<bool> {
        self.has_many()
    }

    pub fn has_many(&self) -> CollectionResult<bool> {
        self.has_many_where(|_, _| true)
    }

    pub fn has_many_where(
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
        self.get_iterator().filter(move |entry| match entry {
            Ok((key, value)) => predicate(value, key),
            Err(_) => true,
        })
    }
}
