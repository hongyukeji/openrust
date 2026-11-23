use std::{
    cell::RefCell,
    collections::VecDeque,
    fmt,
    future::Future,
    sync::{Arc, Mutex},
};

/// 中奖机会支持整数比值和浮点概率，避免大整数先转换为浮点数。
#[derive(Clone, Copy, Debug)]
pub enum LotteryChance {
    Integer(i128),
    Float(f64),
}
macro_rules! integer_chance {
    ($($ty:ty),+) => { $(impl From<$ty> for LotteryChance {
        fn from(value: $ty) -> Self { Self::Integer(value.into()) }
    })+ };
}
integer_chance!(i32, i64, u32, u64);
impl From<f64> for LotteryChance {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

type ResultFactory = dyn Fn(LotteryChance, Option<u64>) -> bool + Send + Sync;
type FactoryState = RefCell<Option<Arc<ResultFactory>>>;
tokio::task_local! { static RESULTS: FactoryState; }
thread_local! { static THREAD_RESULTS: FactoryState = const { RefCell::new(None) }; }

/// 概率选择与两分支回调。测试控制应放在 scope 或 scopeSync 内。
#[derive(Clone, Copy, Debug)]
pub struct Lottery<W = fn() -> bool, L = fn() -> bool> {
    chances: LotteryChance,
    out_of: Option<u64>,
    winner: W,
    loser: L,
}

#[derive(Clone, Copy, Debug)]
pub struct InvalidLottery;
impl fmt::Display for InvalidLottery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("概率必须为有限数值，无分母的浮点概率不能大于 1，分母必须大于 0")
    }
}
impl std::error::Error for InvalidLottery {}

#[allow(non_snake_case)]
impl Lottery {
    pub fn odds(
        chances: impl Into<LotteryChance>,
        out_of: impl Into<Option<u64>>,
    ) -> Result<Self, InvalidLottery> {
        let chances = chances.into();
        let out_of = out_of.into();
        if out_of == Some(0)
            || matches!(chances, LotteryChance::Float(value) if !value.is_finite() || out_of.is_none() && value > 1.0)
        {
            return Err(InvalidLottery);
        }
        Ok(Self {
            chances,
            out_of,
            winner: || true,
            loser: || false,
        })
    }

    /// 独立异步任务中的控制状态在退出、取消或 panic 后自动恢复。
    pub async fn scope<R>(operation: impl Future<Output = R>) -> R {
        RESULTS.scope(RefCell::new(None), operation).await
    }

    pub fn scopeSync<R>(operation: impl FnOnce() -> R) -> R {
        RESULTS.sync_scope(RefCell::new(None), operation)
    }

    pub fn setResultFactory(
        factory: impl Fn(LotteryChance, Option<u64>) -> bool + Send + Sync + 'static,
    ) {
        set_factory(Some(Arc::new(factory)));
    }

    pub fn alwaysWin() {
        Self::setResultFactory(|_, _| true);
    }
    pub fn alwaysLose() {
        Self::setResultFactory(|_, _| false);
    }
    pub fn determineResultNormally() {
        set_factory(None);
    }
    pub fn determineResultsNormally() {
        Self::determineResultNormally();
    }
    pub fn fix(sequence: impl IntoIterator<Item = bool>) {
        Self::forceResultWithSequence(sequence, None);
    }

    pub fn forceResultWithSequence(
        sequence: impl IntoIterator<Item = bool>,
        when_missing: Option<Arc<ResultFactory>>,
    ) {
        let sequence = Mutex::new(sequence.into_iter().collect::<VecDeque<_>>());
        Self::setResultFactory(move |chances, out_of| {
            let next = sequence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            next.unwrap_or_else(|| {
                when_missing.as_ref().map_or_else(
                    || normal_result(chances, out_of),
                    |callback| callback(chances, out_of),
                )
            })
        });
    }
}

impl<W, L> Lottery<W, L> {
    pub fn winner<F>(self, callback: F) -> Lottery<F, L> {
        Lottery {
            chances: self.chances,
            out_of: self.out_of,
            winner: callback,
            loser: self.loser,
        }
    }
    pub fn loser<F>(self, callback: F) -> Lottery<W, F> {
        Lottery {
            chances: self.chances,
            out_of: self.out_of,
            winner: self.winner,
            loser: callback,
        }
    }
}

impl<R, W: Fn() -> R, L: Fn() -> R> Lottery<W, L> {
    pub fn choose(&self) -> R {
        let factory = RESULTS
            .try_with(|state| state.borrow().clone())
            .unwrap_or_else(|_| {
                if tokio::runtime::Handle::try_current().is_ok() {
                    None
                } else {
                    THREAD_RESULTS.with(|state| state.borrow().clone())
                }
            });
        if factory.map_or_else(
            || normal_result(self.chances, self.out_of),
            |factory| factory(self.chances, self.out_of),
        ) {
            (self.winner)()
        } else {
            (self.loser)()
        }
    }

    #[allow(non_snake_case)]
    pub fn chooseTimes(&self, times: usize) -> Vec<R> {
        (0..times).map(|_| self.choose()).collect()
    }
}

fn set_factory(factory: Option<Arc<ResultFactory>>) {
    if RESULTS
        .try_with(|state| *state.borrow_mut() = factory.clone())
        .is_err()
    {
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "异步运行时中的 Lottery 测试控制必须在 Lottery::scope 内使用"
        );
        THREAD_RESULTS.with(|state| *state.borrow_mut() = factory);
    }
}

fn random() -> u64 {
    use aws_lc_rs::rand::{SecureRandom as _, SystemRandom};
    let mut bytes = [0; 8];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("无法生成安全随机数");
    u64::from_ne_bytes(bytes)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "浮点概率遵循双精度数值比较，整数比值单独精确比较"
)]
fn normal_result(chances: LotteryChance, out_of: Option<u64>) -> bool {
    if let Some(total) = out_of {
        // 拒绝尾部残余区间，保持每个整数机会的概率相同。
        let bound = u64::MAX - u64::MAX % total;
        let value = loop {
            let value = random();
            if value < bound {
                break value % total + 1;
            }
        };
        match chances {
            LotteryChance::Integer(chances) => i128::from(value) <= chances,
            LotteryChance::Float(chances) => value as f64 <= chances,
        }
    } else {
        let value = (random() & i64::MAX as u64) as f64 / i64::MAX as f64;
        match chances {
            LotteryChance::Integer(chances) => value <= chances as f64,
            LotteryChance::Float(chances) => value <= chances,
        }
    }
}
