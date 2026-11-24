//! Built-in fluent rule types.

mod any_of;
mod database_rule;
mod dimensions;
mod email;
mod encoding;
mod enum_rule;
mod exists;
mod file;
mod in_rule;
mod password;
mod unique;

pub use any_of::{AnyOf, AnyOfRules};
pub(super) use database_rule::DatabaseRule;
pub use dimensions::Dimensions;
pub use email::Email;
pub(super) use encoding::EncodingRule;
pub use enum_rule::Enum;
pub use exists::Exists;
pub use file::{File, FileSize};
pub use in_rule::{In, NotIn};
pub use password::Password;
pub use unique::Unique;
pub(super) use unique::{Exclusion, prepare_id};

mod array_rule;
mod constraints;
mod date;
mod numeric;
mod string_rule;
pub use array_rule::ArrayRule;
pub use date::{Date, DateParameter};
pub use numeric::Numeric;
pub use string_rule::StringRule;

mod parameterized;
pub(super) use parameterized::Parameterized;
pub use parameterized::{Distinct, IpAddress, Timezone, Url, Uuid};

mod conditional;
pub use conditional::{
    ExcludeIf, ExcludeUnless, ProhibitedIf, ProhibitedUnless, RequiredIf, RequiredUnless,
};
