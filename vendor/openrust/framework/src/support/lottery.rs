use std::{fmt, num::NonZeroU64};

/// A probability expressed as a number of winning outcomes.
#[derive(Clone, Copy, Debug)]
pub struct Lottery {
    chances: u64,
    out_of: NonZeroU64,
}

/// The number of winning outcomes exceeds the available outcomes.
#[derive(Clone, Copy, Debug)]
pub struct InvalidLottery;

impl fmt::Display for InvalidLottery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("lottery odds must have a positive total and no more winners than outcomes")
    }
}

impl std::error::Error for InvalidLottery {}

impl Lottery {
    pub fn odds(chances: u64, out_of: u64) -> Result<Self, InvalidLottery> {
        let out_of = NonZeroU64::new(out_of).ok_or(InvalidLottery)?;
        if chances > out_of.get() {
            return Err(InvalidLottery);
        }
        Ok(Self { chances, out_of })
    }

    pub fn choose(self) -> bool {
        match self.chances {
            0 => false,
            chances if chances == self.out_of.get() => true,
            chances => fastrand::u64(0..self.out_of.get()) < chances,
        }
    }
}
