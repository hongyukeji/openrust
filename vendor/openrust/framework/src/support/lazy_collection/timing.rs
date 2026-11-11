use std::{
    iter,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

use super::{Entry, LazyCollection, SourceIterator};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn takeUntilTimeout(self, timeout: Instant) -> Self {
        Self::from_iterators(move || -> SourceIterator<'source, T> {
            if Instant::now() >= timeout {
                return Box::new(iter::empty());
            }
            let mut iterator = self.getIterator();
            Box::new(iter::from_fn(move || {
                (Instant::now() < timeout)
                    .then(|| iterator.next())
                    .flatten()
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
                if Instant::now() >= timeout {
                    callback(previous.as_ref().map(|(key, value)| (value, key)));
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
