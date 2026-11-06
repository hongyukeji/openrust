//! 按允许列表解析 API 逗号分隔的 `sort` 查询约定。

use crate::{Error, Result};

/// 一个已校验排序字段的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// 按值从低到高排序。
    Ascending,
    /// 按值从高到低排序。
    Descending,
}

/// [`SortParams`] 允许列表接受的一个字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortField {
    name: Box<str>,
    direction: SortDirection,
}

impl SortField {
    /// 返回已校验的公开字段名。请将其映射为固定 ORM 列；不得将其插入 SQL。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回此字段请求的排序方向。
    pub const fn direction(&self) -> SortDirection {
        self.direction
    }
}

/// 根据应用提供的允许列表校验并排序后的字段列表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SortParams(Vec<SortField>);

impl SortParams {
    /// 根据固定的公开字段名解析 历史决策 008 的 `sort=-created_at,title` 语法。
    ///
    /// 输入缺失或为空表示不指定排序。最多允许 16 个不重复字段，输入长度上限为 256 字节。
    /// 未知或格式错误的字段名会返回 bad request，且不会回显输入内容。
    pub fn parse(value: Option<&str>, allowed: &[&str]) -> Result<Self> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(Self::default());
        };
        if value.len() > 256 {
            return Err(invalid_sort());
        }
        let mut fields = Vec::new();
        for entry in value.split(',') {
            if fields.len() == 16 || entry.is_empty() {
                return Err(invalid_sort());
            }
            let (direction, name) = if let Some(name) = entry.strip_prefix('-') {
                (SortDirection::Descending, name)
            } else {
                (SortDirection::Ascending, entry)
            };
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
                || !allowed.contains(&name)
                || fields.iter().any(|field: &SortField| field.name() == name)
            {
                return Err(invalid_sort());
            }
            fields.push(SortField {
                name: name.into(),
                direction,
            });
        }
        Ok(Self(fields))
    }

    /// 按请求中的顺序返回已校验字段。
    pub fn fields(&self) -> &[SortField] {
        &self.0
    }
}

fn invalid_sort() -> Error {
    Error::BadRequest("sort contains an unsupported or invalid field".into())
}

#[cfg(test)]
mod tests {
    use super::SortDirection;
    use super::SortParams;

    #[test]
    fn parses_only_unique_allowlisted_fields_in_order() {
        let result = SortParams::parse(Some("-created_at,title"), &["created_at", "title"]);
        assert!(result.is_ok());
        if let Ok(parsed) = result {
            assert_eq!(parsed.fields()[0].name(), "created_at");
            assert_eq!(parsed.fields()[0].direction(), SortDirection::Descending);
            assert_eq!(parsed.fields()[1].name(), "title");
            assert_eq!(parsed.fields()[1].direction(), SortDirection::Ascending);
        }
    }

    #[test]
    fn rejects_unknown_duplicate_malformed_and_unbounded_fields() {
        let too_long = "a".repeat(257);
        for value in [
            "password_hash",
            "title,title",
            "-",
            "title,,created_at",
            "title;drop_table",
            "a,b,c",
            &too_long,
        ] {
            assert!(SortParams::parse(Some(value), &["title", "created_at", "a", "b"]).is_err());
        }
    }
}
