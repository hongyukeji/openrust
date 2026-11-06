use crate::ext::ustr::UStr;
use crate::protocol::text::ColumnFlags;
use crate::{MySql, MySqlTypeInfo};
pub(crate) use sqlx_core::column::*;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "offline", derive(serde::Serialize, serde::Deserialize))]
pub struct MySqlColumn {
    pub(crate) ordinal: usize,
    pub(crate) name: UStr,
    pub(crate) type_info: MySqlTypeInfo,

    #[cfg_attr(feature = "offline", serde(default))]
    pub(crate) origin: ColumnOrigin,

    #[allow(unused)]
    #[cfg_attr(feature = "offline", serde(skip))]
    pub(crate) flags: Option<ColumnFlags>,

    #[cfg_attr(feature = "offline", serde(default))]
    pub(crate) decimals: Option<u8>,
}

impl MySqlColumn {
    /// 返回服务器原始 decimals 字段；旧离线元数据未知时为 None。
    ///
    /// 时间类型使用它表示小数秒位数；浮点类型的 31 等协议值须由调用方解释。
    pub fn fractional_digits(&self) -> Option<u8> {
        self.decimals
    }
}

impl Column for MySqlColumn {
    type Database = MySql;

    fn ordinal(&self) -> usize {
        self.ordinal
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn type_info(&self) -> &MySqlTypeInfo {
        &self.type_info
    }

    fn origin(&self) -> ColumnOrigin {
        self.origin.clone()
    }
}
