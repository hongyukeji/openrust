//! Request 与 Fluent 的同一组标量读取方法；只展开接口，不复制转换或路径语义。

macro_rules! typed_methods {
    ($field:ident) => {
        /// 读取完整数值或默认 0，按固定 PHP 规则限制范围并保留类型标签。
        pub fn clamp(
            &self,
            key: &str,
            min: crate::support::InputNumber,
            max: crate::support::InputNumber,
        ) -> crate::Result<crate::support::InputNumber> {
            self.clamp_or(key, min, max, crate::support::InputNumber::Integer(0))
        }
        /// 缺失或非完整数值时先采用默认值；反向边界和 NaN 不 panic。
        pub fn clamp_or(
            &self,
            key: &str,
            min: crate::support::InputNumber,
            max: crate::support::InputNumber,
            default: crate::support::InputNumber,
        ) -> crate::Result<crate::support::InputNumber> {
            let value = crate::support::data::checked_get(&self.$field, key)?;
            Ok(value
                .as_ref()
                .and_then(crate::support::InputNumber::from_value)
                .unwrap_or(default)
                .clamp(min, max))
        }
        /// 读取 backed enum；缺失、未 filled 或无匹配 case 返回 None。
        pub fn r#enum<E: crate::support::BackedEnum>(&self, key: &str) -> crate::Result<Option<E>> {
            let value = crate::support::data::checked_get(&self.$field, key)?;
            let Some(value) = value else {
                return Ok(None);
            };
            if crate::support::typed::empty_string(&value) {
                return Ok(None);
            }
            Ok(E::try_from_backing(
                <E::Backing as crate::support::EnumBacking>::from_input(&value)?,
            ))
        }
        /// `enum` 的 Rust 关键字便利别名；行为完全相同。
        pub fn enum_value<E: crate::support::BackedEnum>(
            &self,
            key: &str,
        ) -> crate::Result<Option<E>> {
            self.r#enum(key)
        }
        /// 没有合法 case 时返回给定默认值；不吞掉输入类型错误。
        pub fn enum_or<E: crate::support::BackedEnum>(
            &self,
            key: &str,
            default: E,
        ) -> crate::Result<E> {
            self.enum_or_else(key, || default)
        }
        /// 仅没有合法 case 时执行默认闭包。
        pub fn enum_or_else<E: crate::support::BackedEnum>(
            &self,
            key: &str,
            default: impl FnOnce() -> E,
        ) -> crate::Result<E> {
            Ok(self.r#enum(key)?.unwrap_or_else(default))
        }
        /// 读取保留原键、顺序与重复值的 enum 集合。
        ///
        /// 此集合输出路径要求 Serialize，以验证转换后资源预算；单值 enum 不受此约束。
        /// 这是有界集合实现的容量约束。
        pub fn enums<E: crate::support::BackedEnum + serde::Serialize>(
            &self,
            key: &str,
        ) -> crate::Result<crate::support::Collection<E>> {
            let value = crate::support::data::checked_get(&self.$field, key)?;
            let Some(value) = value else {
                return Ok(crate::support::Collection::default());
            };
            if crate::support::typed::empty_string(&value) {
                return Ok(crate::support::Collection::default());
            }
            let source = crate::support::Collection::from_value(value)?;
            let mut entries = Vec::new();
            for (key, value) in source {
                if let Some(case) = E::try_from_backing(
                    <E::Backing as crate::support::EnumBacking>::from_input(&value)?,
                ) {
                    entries.push((key, case));
                }
            }
            crate::support::Collection::try_from_entries(entries)
        }
        /// 按 PHP `(string)` 转换输入；缺失与 null 均返回空字符串。
        pub fn string(&self, key: &str) -> crate::Result<crate::support::Stringable> {
            self.string_or(key, serde_json::Value::Null)
        }
        /// 仅缺失时转换默认值；显式 null 不采用默认值。
        pub fn string_or(
            &self,
            key: &str,
            default: impl Into<serde_json::Value>,
        ) -> crate::Result<crate::support::Stringable> {
            crate::support::data::read_scalar(
                &self.$field,
                key,
                |value| crate::support::Stringable::from(crate::support::scalars::string(value)),
                || {
                    let value = default.into();
                    crate::support::data::validate_limits(&value)?;
                    Ok(crate::support::Stringable::from(
                        crate::support::scalars::string(&value),
                    ))
                },
            )
        }
        /// `string` 的同义入口。
        pub fn str(&self, key: &str) -> crate::Result<crate::support::Stringable> {
            self.string(key)
        }
        /// `string_or` 的同义入口。
        pub fn str_or(
            &self,
            key: &str,
            default: impl Into<serde_json::Value>,
        ) -> crate::Result<crate::support::Stringable> {
            self.string_or(key, default)
        }
        /// 按 PHP 64 位整数转换读取；缺失默认 0。
        pub fn integer(&self, key: &str) -> crate::Result<i64> {
            self.integer_or(key, 0)
        }
        /// 仅缺失时采用整数默认值；null 和非法数字按 PHP 转为 0。
        pub fn integer_or(&self, key: &str, default: i64) -> crate::Result<i64> {
            crate::support::data::read_scalar(
                &self.$field,
                key,
                crate::support::scalars::integer,
                || Ok(default),
            )
        }
        /// 按 PHP 浮点转换读取；缺失默认 0.0。
        pub fn float(&self, key: &str) -> crate::Result<f64> {
            self.float_or(key, 0.0)
        }
        /// 仅缺失时采用浮点默认值，保留 PHP 数值前缀语义。
        pub fn float_or(&self, key: &str, default: f64) -> crate::Result<f64> {
            crate::support::data::read_scalar(
                &self.$field,
                key,
                crate::support::scalars::float,
                || Ok(default),
            )
        }
        /// 按 PHP `FILTER_VALIDATE_BOOLEAN` 读取；缺失默认 false。
        pub fn boolean(&self, key: &str) -> crate::Result<bool> {
            self.boolean_or(key, false)
        }
        /// 仅缺失时采用布尔默认值；数组和无法识别的字符串为 false。
        pub fn boolean_or(&self, key: &str, default: bool) -> crate::Result<bool> {
            crate::support::data::read_scalar(
                &self.$field,
                key,
                crate::support::scalars::boolean,
                || Ok(default),
            )
        }
    };
}
pub(crate) use typed_methods;

macro_rules! bag_methods {
    ($field:ident) => {
        /// 读取一个值并按 PHP 数组转换；缺失与 null 返回空数组。
        pub fn array(&self, key: &str) -> crate::Result<serde_json::Value> {
            let value = crate::support::data::cast_json(
                crate::support::data::checked_get(&self.$field, key)?
                    .unwrap_or(serde_json::Value::Null),
            );
            crate::support::data::validate_limits(&value)?;
            Ok(value)
        }
        /// 返回全部数据的 PHP 数组快照。
        pub fn array_all(&self) -> serde_json::Value {
            self.$field.clone()
        }
        /// 仅保留选定路径，等价于数组形式的 key 参数。
        pub fn array_only(&self, keys: &[&str]) -> crate::Result<serde_json::Value> {
            self.only(keys)
        }
        /// 将一个输入值包装为保键集合。
        pub fn collect(&self, key: &str) -> crate::Result<crate::support::Collection> {
            crate::support::Collection::from_value(self.array(key)?)
        }
        /// 将全部数据包装为保键集合。
        pub fn collect_all(&self) -> crate::Result<crate::support::Collection> {
            crate::support::Collection::from_value(self.array_all())
        }
        /// 将选定路径包装为保键集合。
        pub fn collect_only(&self, keys: &[&str]) -> crate::Result<crate::support::Collection> {
            crate::support::Collection::from_value(self.only(keys)?)
        }
    };
}
pub(crate) use bag_methods;

// 与 fixed Data::isEmptyString 相同；不对 bool/array 使用标量真值。
pub(crate) fn empty_string(value: &serde_json::Value) -> bool {
    !matches!(
        value,
        serde_json::Value::Bool(_) | serde_json::Value::Array(_) | serde_json::Value::Object(_)
    ) && crate::support::scalars::string(value)
        .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0'])
        .is_empty()
}
