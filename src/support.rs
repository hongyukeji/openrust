//! 共享的字符串、保键集合与动态属性对象；请求输入复用相同有界数据内核。

mod backed_enum;
pub mod calendar;
mod collection;
pub(crate) mod data;
mod finite;
#[cfg(feature = "session")]
pub(crate) use finite::Finite;
mod fluent;
mod number;
pub(crate) mod php_variables;
pub(crate) mod scalars;
pub(crate) mod stringable;
pub(crate) mod typed;

pub use backed_enum::{BackedEnum, EnumBacking};
pub use calendar::{
    CalendarClock, CalendarContext, CalendarInstant, DateOptions, FixedClock, InputDate,
    InputInterval, IntervalError, IntervalUnit, ResolvedTimezone, SystemClock, TimezoneInput,
};
pub use collection::{ArrayKey, Collection};
pub use fluent::Fluent;
pub use number::InputNumber;
pub use stringable::Stringable;
