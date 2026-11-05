//! 共享的字符串、保键集合与动态属性对象；请求输入复用相同有界数据内核。

mod backed_enum;
pub mod calendar;
mod collection;
pub(crate) mod data;
mod data_value;
mod finite;
#[cfg(feature = "session")]
pub(crate) use finite::Finite;
mod fluent;
mod message_bag;
mod number;
pub(crate) mod php_variables;
pub(crate) mod scalars;
pub(crate) mod stringable;
pub(crate) mod typed;
mod view_error_bag;

pub use backed_enum::{BackedEnum, EnumBacking};
pub use calendar::{
    CalendarClock, CalendarContext, CalendarInstant, DateOptions, FixedClock, InputDate,
    InputInterval, IntervalError, IntervalUnit, ResolvedTimezone, SystemClock, TimezoneInput,
};
pub use collection::{ArrayKey, Collection};
pub use data_value::{DataObject, DataValue, ObjectEncoding};
pub use fluent::Fluent;
pub use message_bag::{
    MessageBag, MessageBagContract, MessageInput, MessageJsonOptions, MessageKeys, MessageLookup,
    MessageProvider, MessageValue,
};
pub use number::InputNumber;
pub use view_error_bag::{BagHandle, BagRead, BagWrite, ViewErrorBag};

pub use stringable::Stringable;

#[cfg(test)]
#[path = "../tests/support.rs"]
mod support_contract_tests;

mod strings;
pub use strings::Str;
