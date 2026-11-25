// 第三方许可见 components/LICENSE。
use crate::{console::ConsoleError, support::BackedEnum};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 任务状态的整数标识；转换失败不会回退到成功。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum TaskResult {
    Success = 1,
    Failure = 2,
    Skipped = 3,
}
impl TaskResult {
    pub const fn value(self) -> i64 {
        self as i64
    }
    pub const fn cases() -> [Self; 3] {
        [Self::Success, Self::Failure, Self::Skipped]
    }
}
impl BackedEnum for TaskResult {
    type Backing = i64;
    fn try_from_backing(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::Success),
            2 => Some(Self::Failure),
            3 => Some(Self::Skipped),
            _ => None,
        }
    }
}
impl TryFrom<i64> for TaskResult {
    type Error = ConsoleError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::try_from_backing(value).ok_or_else(|| ConsoleError::new("无效的任务状态标识"))
    }
}
impl From<TaskResult> for i64 {
    fn from(value: TaskResult) -> Self {
        value.value()
    }
}
impl Serialize for TaskResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.value())
    }
}
impl<'de> Deserialize<'de> for TaskResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(i64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
