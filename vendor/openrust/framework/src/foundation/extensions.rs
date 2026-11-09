//! Provider 贡献目录。目录锁仅用于定位独立类型槽。

use crate::{Error, Result};
use parking_lot::{ArcRwLockReadGuard, ArcRwLockWriteGuard, RawRwLock, RwLock};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

type Slot<T> = OnceLock<Arc<RwLock<T>>>;
type ErasedSlot = Arc<dyn Any + Send + Sync>;
type Entries = HashMap<TypeId, ErasedSlot>;

#[derive(Clone, Default)]
pub(crate) struct Extensions(Arc<Mutex<Entries>>);

impl Extensions {
    fn entries(&self) -> MutexGuard<'_, Entries> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn get<T: Send + Sync + 'static>(&self) -> Option<ExtensionRef<'_, T>> {
        let slot = self.entries().get(&TypeId::of::<T>())?.clone();
        let slot = slot.downcast::<Slot<T>>().ok()?;
        Some(ExtensionRef(slot.get()?.read_arc(), PhantomData))
    }

    pub(crate) fn get_or_default<T: Default + Send + Sync + 'static>(
        &self,
    ) -> Result<ExtensionMut<'_, T>> {
        let slot = self
            .entries()
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(Slot::<T>::new()))
            .clone();
        let slot = slot
            .downcast::<Slot<T>>()
            .map_err(|_| Error::custom("extension_type_mismatch", "领域贡献类型内部不匹配"))?;
        // 不在目录锁内执行扩展的 Default。
        let value = slot.get_or_init(|| Arc::new(RwLock::new(T::default())));
        Ok(ExtensionMut(value.write_arc(), PhantomData))
    }

    pub(crate) fn take(&self) -> impl Iterator<Item = ErasedSlot> + use<> {
        std::mem::take(&mut *self.entries()).into_values()
    }
}

/// 读取一种 Provider 贡献的短期借用。异步工作前先提取所需数据并释放借用。
pub struct ExtensionRef<'a, T>(ArcRwLockReadGuard<RawRwLock, T>, PhantomData<&'a T>);

// 让借用持续到持锁值析构，避免字段引用结束时提早释放调用者的 Rust 借用。
impl<T> Drop for ExtensionRef<'_, T> {
    fn drop(&mut self) {}
}

impl<T> Deref for ExtensionRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// 修改一种 Provider 贡献的短期借用。释放后才能再次访问同一种贡献。
pub struct ExtensionMut<'a, T>(ArcRwLockWriteGuard<RawRwLock, T>, PhantomData<&'a mut T>);

impl<T> Drop for ExtensionMut<'_, T> {
    fn drop(&mut self) {}
}

impl<T> Deref for ExtensionMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for ExtensionMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_write_to_one_extension_does_not_hold_the_directory_lock() -> Result<()> {
        let extensions = Extensions::default();
        let mut notes = extensions.get_or_default::<Vec<String>>()?;
        notes.push("共享".to_owned());
        let (finished, completion) = mpsc::sync_channel(1);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let result = extensions.get_or_default::<usize>().map(|mut count| {
                    *count += 1;
                });
                let _ = finished.send(result);
            });
            let completed = completion.recv_timeout(Duration::from_secs(2));
            drop(notes);
            completed.map_err(Error::internal)??;
            Ok::<(), Error>(())
        })?;
        assert_eq!(extensions.get::<usize>().map(|value| *value), Some(1));
        assert_eq!(
            extensions.get::<Vec<String>>().map(|value| value.len()),
            Some(1)
        );
        Ok(())
    }
}
