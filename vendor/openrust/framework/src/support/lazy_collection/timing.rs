use std::{
    iter,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

use super::{Entry, LazyCollection, SourceIterator};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn takeUntilTimeout(self, timeout: Instant) -> Self {
        Self::from_iterators(move || -> SourceIterator<'source, T> {
            if Instant::now() >= timeout {
                return Box::new(iter::empty());
            }
            let mut iterator = self.getIterator();
            let mut yielded = false;
            Box::new(iter::from_fn(move || {
                if yielded && Instant::now() >= timeout {
                    return None;
                }
                let entry = iterator.next()?;
                yielded = true;
                Some(entry)
            }))
        })
    }

    pub fn takeUntilTimeoutWith(
        self,
        timeout: Instant,
        callback: impl Fn(Option<(&T, &super::ArrayKey)>) + 'source,
    ) -> Self
    where
        T: Clone,
    {
        let callback = Rc::new(callback);
        Self::from_iterators(move || -> SourceIterator<'source, T> {
            if Instant::now() >= timeout {
                callback(None);
                return Box::new(iter::empty());
            }
            let callback = Rc::clone(&callback);
            let mut iterator = self.getIterator();
            let mut previous: Option<Entry<T>> = None;
            Box::new(iter::from_fn(move || {
                if let Some((key, value)) = previous.as_ref()
                    && Instant::now() >= timeout
                {
                    callback(Some((value, key)));
                    return None;
                }
                let entry = iterator.next()?;
                previous = entry.as_ref().ok().cloned();
                Some(entry)
            }))
        })
    }

    pub fn throttle(self, interval: Duration) -> Self {
        Self::from_iterators(move || {
            let mut iterator = self.getIterator();
            let mut previous: Option<Instant> = None;
            iter::from_fn(move || {
                if let Some(start) = previous {
                    let remaining = interval.saturating_sub(start.elapsed());
                    if !remaining.is_zero() {
                        thread::sleep(remaining);
                    }
                }
                let entry = iterator.next()?;
                previous = entry.as_ref().ok().map(|_| Instant::now());
                Some(entry)
            })
        })
    }

    pub fn withHeartbeat(self, interval: Duration, callback: impl Fn() + 'source) -> Self {
        let callback = Rc::new(callback);
        Self::from_iterators(move || {
            let callback = Rc::clone(&callback);
            let mut start = Instant::now();
            let mut iterator = self.getIterator();
            iter::from_fn(move || {
                let entry = iterator.next()?;
                let now = Instant::now();
                if entry.is_ok() && now.duration_since(start) >= interval {
                    callback();
                    start = now;
                }
                Some(entry)
            })
        })
    }
}
