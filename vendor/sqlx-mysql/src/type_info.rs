use std::fmt::{self, Display, Formatter};

use crate::collation::Collation;
use crate::protocol::text::{ColumnDefinition, ColumnFlags, ColumnType};
pub(crate) use sqlx_core::type_info::*;

/// Type information for a MySql type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "offline", derive(serde::Serialize, serde::Deserialize))]
pub struct MySqlTypeInfo {
    pub(crate) r#type: ColumnType,
    pub(crate) flags: ColumnFlags,
    pub(crate) collation: Collation,

    // [max_size] for integer types, this is (M) in BIT(M) or TINYINT(M)
    #[cfg_attr(feature = "offline", serde(default))]
    pub(crate) max_size: Option<u32>,
}

impl MySqlTypeInfo {
    /// 返回服务器列元数据中的原始 MySQL 协议类型编号，不经名称兼容映射。
    pub fn wire_type_id(&self) -> u8 {
        self.r#type as u8
    }

    /// 服务器是否声明该数值列为 unsigned。
    pub fn is_unsigned(&self) -> bool {
        self.flags.contains(ColumnFlags::UNSIGNED)
    }

    /// 服务器是否声明该数值列为 ZEROFILL。
    pub fn is_zerofill(&self) -> bool {
        self.flags.contains(ColumnFlags::ZEROFILL)
    }

    /// 返回协议 column_length；未知离线元数据保持 None，不猜测显示宽度。
    pub fn display_width(&self) -> Option<u32> {
        self.max_size
    }

    pub(crate) const fn binary(ty: ColumnType) -> Self {
        Self {
            r#type: ty,
            flags: ColumnFlags::BINARY,
            collation: Collation::BINARY,
            max_size: None,
        }
    }

    #[doc(hidden)]
    pub const fn __enum() -> Self {
        // Newer versions of MySQL seem to expect that a parameter binding of `MYSQL_TYPE_ENUM`
        // means that the value is encoded as an integer.
        //
        // For "strong" enums inputted as strings, we need to specify this type instead
        // for wider compatibility. This works on all covered versions of MySQL and MariaDB.
        //
        // Annoyingly, MySQL's developer documentation doesn't really explain this anywhere;
        // this had to be determined experimentally.
        Self {
            r#type: ColumnType::String,
            flags: ColumnFlags::ENUM,
            collation: Collation::UTF8MB4_GENERAL_CI,
            max_size: None,
        }
    }

    #[doc(hidden)]
    pub fn __type_feature_gate(&self) -> Option<&'static str> {
        match self.r#type {
            ColumnType::Date | ColumnType::Time | ColumnType::Timestamp | ColumnType::Datetime => {
                Some("time")
            }

            ColumnType::Json => Some("json"),
            ColumnType::NewDecimal => Some("bigdecimal"),

            _ => None,
        }
    }

    pub(crate) fn from_column(column: &ColumnDefinition) -> Self {
        Self {
            r#type: column.r#type,
            flags: column.flags,
            collation: column.collation,
            max_size: Some(column.max_size),
        }
    }
}

impl Display for MySqlTypeInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    #[test]
    fn raw_metadata_keeps_tiny_wire_type_and_numeric_display_flags() {
        let mut info = MySqlTypeInfo::binary(ColumnType::Tiny);
        info.max_size = Some(1);
        assert_eq!(info.wire_type_id(), 1);
        assert!(!info.is_unsigned());
        assert!(!info.is_zerofill());
        assert_eq!(info.display_width(), Some(1));

        info.flags |= ColumnFlags::UNSIGNED | ColumnFlags::ZEROFILL;
        info.max_size = Some(20);
        assert_eq!(info.wire_type_id(), 1);
        assert!(info.is_unsigned());
        assert!(info.is_zerofill());
        assert_eq!(info.display_width(), Some(20));
        assert_eq!(
            MySqlTypeInfo::binary(ColumnType::LongLong).display_width(),
            None
        );
    }
}

impl TypeInfo for MySqlTypeInfo {
    fn is_null(&self) -> bool {
        matches!(self.r#type, ColumnType::Null)
    }

    fn name(&self) -> &str {
        self.r#type.name(self.flags, self.max_size)
    }
}

impl PartialEq<MySqlTypeInfo> for MySqlTypeInfo {
    fn eq(&self, other: &MySqlTypeInfo) -> bool {
        if self.r#type != other.r#type {
            return false;
        }

        match self.r#type {
            ColumnType::Tiny
            | ColumnType::Short
            | ColumnType::Long
            | ColumnType::Int24
            | ColumnType::LongLong => {
                return self.flags.contains(ColumnFlags::UNSIGNED)
                    == other.flags.contains(ColumnFlags::UNSIGNED);
            }

            // for string types, check that our charset matches
            ColumnType::VarChar
            | ColumnType::Blob
            | ColumnType::TinyBlob
            | ColumnType::MediumBlob
            | ColumnType::LongBlob
            | ColumnType::String
            | ColumnType::VarString
            | ColumnType::Enum => {
                return self.flags == other.flags;
            }
            _ => {}
        }

        true
    }
}

impl Eq for MySqlTypeInfo {}
