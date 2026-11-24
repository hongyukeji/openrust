//! 日期工厂与应用时钟；日期值沿用现有带时区的 Date。
use super::{Date, DateError, DateTimezone, TestClock};
use crate::{
    container::{Container, ServiceKey},
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Facade},
};
use std::sync::{Arc, OnceLock, Weak};

/// 日期创建与测试时钟入口。应用之间不共享冻结时间。
#[derive(Default)]
pub struct DateFactory {
    clock: Arc<TestClock>,
    owner: Option<Weak<Container>>,
}
impl DateFactory {
    fn zone(&self, timezone: Option<DateTimezone>) -> Result<DateTimezone, DateError> {
        if let Some(timezone) = timezone {
            return Ok(timezone);
        }
        let Some(owner) = &self.owner else {
            return Ok(DateTimezone::UTC);
        };
        let owner = owner.upgrade().ok_or(DateError::Configuration)?;
        let config = owner
            .get::<dyn Repository>("config")
            .map_err(|_| DateError::Configuration)?;
        match config.get("app.timezone") {
            None => Ok(DateTimezone::UTC),
            Some(value) => Date::timezone(value.as_str().ok_or(DateError::InvalidTimezone)?),
        }
    }
    pub fn now(&self, timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        Ok(Date::from_utc(
            self.clock.now().unwrap_or_else(jiff::Timestamp::now),
            self.zone(timezone)?,
        ))
    }
    pub fn today(&self, timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        self.now(timezone)?.start_of_day()
    }
    pub fn tomorrow(&self, timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        self.today(timezone)?
            .plus(super::days(1.0)?, super::MonthOverflow::Carry)
    }
    pub fn yesterday(&self, timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        self.today(timezone)?
            .minus(super::days(1.0)?, super::MonthOverflow::Carry)
    }
    pub fn get_test_now(&self) -> Option<Date> {
        self.clock.date()
    }
    pub fn has_test_now(&self) -> bool {
        self.get_test_now().is_some()
    }
    pub fn set_test_now(&self, time: Option<Date>) {
        self.clock.set_date(time);
    }
    /// 冻结值在本异步执行域可见；退出清除测试时间，包括错误、panic和取消。
    pub async fn with_test_now<F: Future>(&self, time: Option<Date>, future: F) -> F::Output {
        let _clear = ClearTestNow(self.clock.clone());
        self.clock.clone().scope_date(time, future).await
    }
    pub fn instance(&self, date: &Date) -> Date {
        date.clone()
    }
}

struct ClearTestNow(Arc<TestClock>);
impl Drop for ClearTestNow {
    fn drop(&mut self) {
        self.0.set_date(None);
    }
}

pub(crate) fn standalone_clock() -> Arc<TestClock> {
    static CLOCK: OnceLock<Arc<TestClock>> = OnceLock::new();
    CLOCK.get_or_init(Arc::default).clone()
}
fn standalone() -> Arc<DateFactory> {
    static FACTORY: OnceLock<Arc<DateFactory>> = OnceLock::new();
    FACTORY
        .get_or_init(|| {
            Arc::new(DateFactory {
                clock: standalone_clock(),
                owner: None,
            })
        })
        .clone()
}
// 通过既有门面缓存解析应用绑定；无应用时才使用默认工厂。
struct BoundDate;
impl Facade for BoundDate {
    type Service = DateFactory;
    fn get_facade_accessor() -> ServiceKey {
        "date".into()
    }
}
pub struct DateFacade;
impl Facade for DateFacade {
    type Service = DateFactory;
    fn get_facade_accessor() -> ServiceKey {
        "date".into()
    }
    fn get_facade_root() -> Result<Arc<DateFactory>, BindingResolutionError> {
        match BoundDate::get_facade_root() {
            Err(BindingResolutionError::FacadeNotSet) => Ok(standalone()),
            result => result,
        }
    }
}
impl DateFacade {
    pub fn now(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        Self::factory()?.now(timezone)
    }
    pub fn today(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        Self::factory()?.today(timezone)
    }
    pub fn tomorrow(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        Self::factory()?.tomorrow(timezone)
    }
    pub fn yesterday(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
        Self::factory()?.yesterday(timezone)
    }
    fn factory() -> Result<Arc<DateFactory>, DateError> {
        Self::get_facade_root().map_err(|_| DateError::Configuration)
    }
    pub fn get_test_now() -> Result<Option<Date>, DateError> {
        Ok(Self::factory()?.get_test_now())
    }
    pub fn has_test_now() -> Result<bool, DateError> {
        Ok(Self::factory()?.has_test_now())
    }
    pub fn set_test_now(time: Option<Date>) -> Result<(), DateError> {
        Self::factory()?.set_test_now(time);
        Ok(())
    }
    pub async fn with_test_now<F: Future>(
        time: Option<Date>,
        future: F,
    ) -> Result<F::Output, DateError> {
        Ok(Self::factory()?.with_test_now(time, future).await)
    }
    pub fn instance(date: &Date) -> Result<Date, DateError> {
        Ok(Self::factory()?.instance(date))
    }
}

pub(crate) struct DateServiceProvider;
impl ServiceProvider for DateServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = app.container_handle();
            if !owner.bound::<TestClock>() {
                owner.singleton::<TestClock, _>(|_| Ok(Arc::default()))?;
            }
            let weak = Arc::downgrade(&owner);
            owner.singleton::<DateFactory, _>(move |container| {
                Ok(Arc::new(DateFactory {
                    clock: container.make::<TestClock>()?,
                    owner: Some(weak.clone()),
                }))
            })?;
            owner.alias(ServiceKey::typed::<DateFactory>(), "date")?;
            Ok(())
        })
    }
}
