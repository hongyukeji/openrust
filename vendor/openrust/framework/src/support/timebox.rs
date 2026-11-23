use std::time::Duration;
use tokio::time::Instant;

use super::{Sleep, SleepFake};

/// Holds a completed return value until the configured minimum time has elapsed.
#[derive(Default)]
pub struct Timebox {
    early_return: bool,
    fake: Option<SleepFake>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Timebox {
    pub fn using(mut self, fake: &SleepFake) -> Self {
        self.fake = Some(fake.clone());
        self
    }

    pub fn returnEarly(&mut self) -> &mut Self {
        self.early_return = true;
        self
    }

    pub fn dontReturnEarly(&mut self) -> &mut Self {
        self.early_return = false;
        self
    }

    pub async fn call<T>(
        &mut self,
        callback: impl AsyncFnOnce(&mut Self) -> T,
        minimum: Duration,
    ) -> T {
        let start = Instant::now();
        let result = callback(self).await;
        let remainder = minimum.saturating_sub(start.elapsed());
        if !self.early_return && !remainder.is_zero() {
            let mut delay = Sleep::r#for(remainder);
            if let Some(fake) = &self.fake {
                delay = delay.using(fake);
            }
            delay.wait().await;
        }
        result
    }
}
