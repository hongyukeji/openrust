use crate::support::ArrayKey;
/// 可计数且保留键的借用迭代，不消费或复制原集合。
pub trait ProgressIterable {
    type Item;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &Self::Item)>;
}
impl<T> ProgressIterable for [T] {
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.iter().enumerate().map(|(index, value)| {
            (
                ArrayKey::Integer(i64::try_from(index).expect("集合下标位于整数范围内")),
                value,
            )
        })
    }
}
impl<T> ProgressIterable for Vec<T> {
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.as_slice().progress_iter()
    }
}
impl<T, const N: usize> ProgressIterable for [T; N] {
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.as_slice().progress_iter()
    }
}
impl<I: ProgressIterable + ?Sized> ProgressIterable for &I {
    type Item = I::Item;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &Self::Item)> {
        I::progress_iter(*self)
    }
}
impl<I: ProgressIterable + ?Sized> ProgressIterable for &mut I {
    type Item = I::Item;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &Self::Item)> {
        I::progress_iter(&**self)
    }
}
impl<K: Clone + Into<ArrayKey> + Ord, T> ProgressIterable for std::collections::BTreeMap<K, T> {
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.iter()
            .map(|(key, value)| (key.clone().into().normalized(), value))
    }
}
impl<K: Clone + Into<ArrayKey> + Eq + std::hash::Hash, T, S: std::hash::BuildHasher>
    ProgressIterable for indexmap::IndexMap<K, T, S>
{
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.iter()
            .map(|(key, value)| (key.clone().into().normalized(), value))
    }
}
impl<T> ProgressIterable for crate::support::Collection<T> {
    type Item = T;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &T)> {
        self.iter().map(|(key, value)| (key.clone(), value))
    }
}
impl<M: crate::database::eloquent::Model> ProgressIterable
    for crate::database::eloquent::Collection<M>
{
    type Item = M;
    fn progress_iter(&self) -> impl ExactSizeIterator<Item = (ArrayKey, &M)> {
        self.entries()
    }
}

/// 显式选择原地处理；默认只读迭代不会改变调用方集合中的值。
pub trait ProgressIterableMut: ProgressIterable {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut Self::Item)>;
}
impl<T> ProgressIterableMut for [T] {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.iter_mut().enumerate().map(|(index, value)| {
            (
                ArrayKey::Integer(i64::try_from(index).expect("集合下标位于整数范围内")),
                value,
            )
        })
    }
}
impl<T> ProgressIterableMut for Vec<T> {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.as_mut_slice().progress_iter_mut()
    }
}
impl<T, const N: usize> ProgressIterableMut for [T; N] {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.as_mut_slice().progress_iter_mut()
    }
}
impl<I: ProgressIterableMut + ?Sized> ProgressIterableMut for &mut I {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut Self::Item)> {
        I::progress_iter_mut(&mut **self)
    }
}
impl<K: Clone + Into<ArrayKey> + Ord, T> ProgressIterableMut for std::collections::BTreeMap<K, T> {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.iter_mut()
            .map(|(key, value)| (key.clone().into().normalized(), value))
    }
}
impl<K: Clone + Into<ArrayKey> + Eq + std::hash::Hash, T, S: std::hash::BuildHasher>
    ProgressIterableMut for indexmap::IndexMap<K, T, S>
{
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.iter_mut()
            .map(|(key, value)| (key.clone().into().normalized(), value))
    }
}
impl<T> ProgressIterableMut for crate::support::Collection<T> {
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut T)> {
        self.iter_mut().map(|(key, value)| (key.clone(), value))
    }
}
impl<M: crate::database::eloquent::Model> ProgressIterableMut
    for crate::database::eloquent::Collection<M>
{
    fn progress_iter_mut(&mut self) -> impl ExactSizeIterator<Item = (ArrayKey, &mut M)> {
        self.entries_mut()
    }
}
