use std::{
    ops::RangeBounds,
    sync::{Mutex, OnceLock},
};

use fake::rand::{SeedableRng, rngs::SmallRng};

pub(super) struct FakerRandom(OnceLock<Mutex<fastrand::Rng>>);

macro_rules! integers {
    ($($ty:ident),+ $(,)?) => {
        $(
            pub(super) fn $ty(&self, range: impl RangeBounds<$ty>) -> $ty {
                match self.0.get() {
                    Some(rng) => rng.lock().unwrap().$ty(range),
                    None => fastrand::$ty(range),
                }
            }
        )+
    };
}

impl FakerRandom {
    pub(super) fn new() -> Self {
        Self(OnceLock::new())
    }

    pub(super) fn seed(&self, seed: Option<u64>) {
        let rng = seed.map_or_else(fastrand::Rng::new, fastrand::Rng::with_seed);
        *self
            .0
            .get_or_init(|| Mutex::new(fastrand::Rng::new()))
            .lock()
            .unwrap() = rng;
    }

    integers!(i32, i64, u8, u16, u32, u64, u128, usize);

    pub(super) fn bool(&self) -> bool {
        match self.0.get() {
            Some(rng) => rng.lock().unwrap().bool(),
            None => fastrand::bool(),
        }
    }

    pub(super) fn f64(&self) -> f64 {
        match self.0.get() {
            Some(rng) => rng.lock().unwrap().f64(),
            None => fastrand::f64(),
        }
    }

    pub(super) fn sample<T: Copy>(&self, values: &[T]) -> T {
        values[self.usize(..values.len())]
    }

    pub(super) fn is_seeded(&self) -> bool {
        self.0.get().is_some()
    }

    pub(super) fn shuffle<T>(&self, values: &mut [T]) {
        match self.0.get() {
            Some(rng) => rng.lock().unwrap().shuffle(values),
            None => fastrand::shuffle(values),
        };
    }

    pub(super) fn fake_rng(&self) -> Option<SmallRng> {
        self.0
            .get()
            .map(|rng| SmallRng::seed_from_u64(rng.lock().unwrap().u64(..)))
    }
}
