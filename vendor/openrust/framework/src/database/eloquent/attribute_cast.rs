//! 基础属性读取转换；原始值与模型读取值分开，Query Processor 不复用此规则。

use super::{AttributeValue, ClassCast, Json, ModelDateContext, ModelMetadata};
use crate::{Error, Result};
use serde_json::Value;
use std::borrow::Cow;

#[path = "attribute_json.rs"]
mod json;

/// Eloquent 的属性转换声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeCast {
    /// PHP64 整数转换，接受字符串数值前缀。
    Integer,
    /// PHP 标量字符串转换；数组转换的默认警告升级为错误。
    String,
    /// PHP 真值转换，不使用 Request 的 `FILTER_VALIDATE_BOOLEAN`。
    Boolean,
    /// PHP 浮点转换；JSON 属性载体无法表示非有限结果时明确拒绝。
    Float,
    /// 读取时按十进制位数定标。参数保留声明的原始文本，缺省参数仅允许 null 读取。
    Decimal(Option<String>),
    /// `array` 与 `json` 按 PHP 关联数组规则读取持久 JSON。
    Array,
    /// 使用 JSON 对象语义读取，保留对象键和嵌套对象形状。
    Object,
    /// 使用关联数组解码，每次模型读取构造一个新的集合。
    Collection,
    /// 使用关联数组解码，赋值时保留可直接输出的 Unicode 字符。
    JsonUnicode,
    /// 赋值时使用同一 `HashManager`；读取及水合不重新哈希。
    Hashed,
    /// 日期对象，读取时取本地午夜。
    Date,
    /// 保留时分秒和微秒的日期对象。
    DateTime,
    /// 本地午夜的不可变日期对象。
    ImmutableDate,
    /// 保留完整时间的不可变日期对象。
    ImmutableDateTime,
    /// 带序列化格式的 `date:…` 或 `datetime:…`。
    CustomDateTime(String),
    /// 带序列化格式的不可变日期。
    ImmutableCustomDateTime(String),
    /// 读取 Unix 秒整数。
    Timestamp,
    /// 由模型显式声明的自定义类转换。
    Class(ClassCast),
    /// 声明模型的原生枚举。
    Enum(super::EnumCast),
    /// 原生加密属性；使用应用 Encrypter 处理持久密文。
    #[cfg(feature = "encryption")]
    Encrypted(super::EncryptedCast),
}

impl AttributeCast {
    /// 声明读取时按十进制位数定标的属性，赋值保留原始值。
    pub fn decimal(scale: u32) -> Self {
        Self::Decimal(Some(scale.to_string()))
    }

    /// 对独立原始值执行读取转换；不运行模型访问器，也不执行赋值哈希。
    pub fn read(&self, value: Option<Value>) -> Result<Option<Value>> {
        if matches!(self, Self::Class(_)) {
            return Err(Error::custom(
                "model_class_cast_context_required",
                "类转换读取需要模型字段和原始属性",
            ));
        }
        if self.is_date_read() {
            return value
                .map(|value| {
                    ModelDateContext::default().read(
                        &ModelMetadata::new("", ""),
                        self,
                        value.into(),
                    )
                })
                .transpose()?
                .map(AttributeValue::try_into_json)
                .transpose();
        }
        cast_read_value(self, value)
    }

    pub(super) fn is_date_read(&self) -> bool {
        self.is_date_storage()
            || matches!(
                self,
                Self::Timestamp | Self::CustomDateTime(_) | Self::ImmutableCustomDateTime(_)
            )
    }

    pub(super) const fn is_json_projection(&self) -> bool {
        matches!(
            self,
            Self::Array | Self::Object | Self::Collection | Self::JsonUnicode
        )
    }

    pub(super) fn is_date_storage(&self) -> bool {
        matches!(
            self,
            Self::Date | Self::DateTime | Self::ImmutableDate | Self::ImmutableDateTime
        )
    }
}

pub(crate) fn cast_read_value(cast: &AttributeCast, raw: Option<Value>) -> Result<Option<Value>> {
    let Some(value) = raw else { return Ok(None) };
    if value.is_null() {
        return Ok(Some(Value::Null));
    }
    let value = match cast {
        AttributeCast::Enum(cast) => cast.read(value.into())?.try_into_json()?,
        #[cfg(feature = "encryption")]
        AttributeCast::Encrypted(cast) => cast.read(value.into())?.into_array_json()?,
        AttributeCast::Integer => {
            if let Value::Number(number) = &value
                && number.as_i64().is_none()
            {
                let number = number.as_f64().ok_or_else(cast_failed)?;
                // PHP native float 的 (int) 越界产生 E_WARNING，固定 PHP 对照将其提升为异常。
                if !number.is_finite()
                    || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0)
                        .contains(&number)
                {
                    return Err(cast_failed());
                }
            }
            Value::from(crate::support::scalars::integer(&value))
        }
        AttributeCast::String => Value::String(scalar_string(&value)?),
        AttributeCast::Boolean => Value::Bool(match &value {
            Value::Bool(value) => *value,
            Value::Number(value) => value.as_f64() != Some(0.0),
            Value::String(value) => !value.is_empty() && value != "0",
            Value::Array(value) => !value.is_empty(),
            Value::Object(value) => !value.is_empty(),
            Value::Null => false,
        }),
        AttributeCast::Float => {
            let number = cast_float_value(&value)?;
            Value::Number(serde_json::Number::from_f64(number).ok_or_else(cast_failed)?)
        }
        AttributeCast::Array | AttributeCast::JsonUnicode => from_json(&value, true)?,
        AttributeCast::Object => from_json(&value, false)?,
        AttributeCast::Collection => collection_items(from_json(&value, true)?),
        AttributeCast::Hashed => value,
        AttributeCast::Decimal(scale) => super::decimal_cast::read(&value, scale.as_deref())?,
        _ => return Err(cast_failed()),
    };
    Ok(Some(value))
}

// 比较阶段保留原生double；只有公开读取输出Value时才拒绝非有限值。
pub(crate) fn cast_float_value(value: &Value) -> Result<f64> {
    match scalar_string(value)?.as_str() {
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        "NaN" => Ok(f64::NAN),
        _ => {
            if let Value::String(text) = value {
                crate::support::scalars::numeric_prefix(text).map_or(Ok(0.0), model_decimal_float)
            } else {
                Ok(crate::support::scalars::float(value))
            }
        }
    }
}

pub(crate) fn array_cast_equivalent(left: &Value, right: &Value) -> Result<bool> {
    if let Some(decoder) = Json::decoder() {
        let left = if skips_decode(left) {
            Value::Null
        } else {
            decoder(left, Some(true))?
        };
        let right = from_json(right, true)?;
        return Ok(super::attribute_store::strict_equal(&left, &right));
    }
    json::equivalent(&scalar_string(left)?, &scalar_string(right)?)
}

pub(crate) fn scalar_string(value: &Value) -> Result<String> {
    if value.is_array() || value.is_object() {
        return Err(cast_failed());
    }
    Ok(crate::support::scalars::string(value))
}

// 固定zend_strtod先忽略指数前导零；超过8位或绝对值>19999时截断，再计算小数位置。
// 不能先约去尾数零或合并小数位，否则两者抵消时会错误绕过这条可观察规则。
fn model_decimal_text(text: &str) -> Cow<'_, str> {
    crate::support::scalars::decimal_text(text)
}

fn model_decimal_float(text: &str) -> Result<f64> {
    model_decimal_text(text)
        .parse::<f64>()
        .map_err(|_| cast_failed())
}

fn skips_decode(value: &Value) -> bool {
    value.is_null() || value.as_str() == Some("")
}

pub(super) fn from_json(value: &Value, associative: bool) -> Result<Value> {
    if skips_decode(value) {
        return Ok(Value::Null);
    }
    Json::decode(value, Some(associative))
}

pub(super) fn decode_default_json(value: &Value, associative: bool) -> Result<Value> {
    let text = scalar_string(value)?;
    decode_default_text(&text, associative)
}

pub(super) fn decode_default_text(text: &str, associative: bool) -> Result<Value> {
    if associative {
        json::decode_associative(text)
    } else {
        json::decode_object(text)
    }
}

#[cfg(feature = "encryption")]
pub(super) fn default_json_equivalent(left: &str, right: &str) -> Result<bool> {
    json::equivalent(left, right)
}

pub(super) fn collection_items(value: Value) -> Value {
    match value {
        value @ (Value::Array(_) | Value::Object(_)) => value,
        Value::Null => Value::Array(Vec::new()),
        value => Value::Array(vec![value]),
    }
}

pub(super) fn encode_array_json(value: &Value) -> Result<Value> {
    encode_projection_json(value, None)
}

pub(super) fn encode_projection_json(value: &Value, cast: Option<&AttributeCast>) -> Result<Value> {
    let flags = if matches!(cast, Some(AttributeCast::JsonUnicode)) {
        Json::UNESCAPED_UNICODE
    } else {
        0
    };
    let encoded = Json::encode(value, flags)?;
    if encoded == Value::Bool(false) {
        return Err(Error::custom(
            "model_json_encoding_failed",
            "模型属性 JSON 编码失败",
        ));
    }
    Ok(encoded)
}

fn cast_failed() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_attribute_cast_failed",
        "模型属性无法按声明规则转换",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integer_cast_accepts_prefixes_and_does_not_apply_query_numeric_filter() -> Result<()> {
        for (raw, expected) in [
            (json!("00042"), json!(42)),
            (json!("123tail"), json!(123)),
            (json!("uuid-key"), json!(0)),
            (json!("18446744073709551615"), json!(i64::MAX)),
            (json!(-1.75), json!(-1)),
            (json!(null), json!(null)),
        ] {
            assert_eq!(
                cast_read_value(&AttributeCast::Integer, Some(raw))?,
                Some(expected)
            );
        }
        assert_eq!(cast_read_value(&AttributeCast::Integer, None)?, None);
        let error = cast_read_value(
            &AttributeCast::Integer,
            Some(json!(9_223_372_036_854_775_808.0)),
        )
        .err()
        .ok_or_else(cast_failed)?;
        assert_eq!(error.code(), "model_attribute_cast_failed");
        assert_eq!(error.status().as_u16(), 500);
        Ok(())
    }

    #[test]
    fn bool_json_and_string_casts_keep_distinct_php_rules() -> Result<()> {
        assert_eq!(
            cast_read_value(&AttributeCast::Boolean, Some(json!("false")))?,
            Some(json!(true))
        );
        assert_eq!(
            cast_read_value(&AttributeCast::Boolean, Some(json!("0")))?,
            Some(json!(false))
        );
        assert_eq!(
            cast_read_value(&AttributeCast::Array, Some(json!("{\"id\":42}")))?,
            Some(json!({"id":42}))
        );
        assert_eq!(
            cast_read_value(&AttributeCast::Array, Some(json!("broken")))?,
            Some(Value::Null)
        );
        assert_eq!(
            cast_read_value(&AttributeCast::String, Some(json!(true)))?,
            Some(json!("1"))
        );
        assert!(cast_read_value(&AttributeCast::String, Some(json!([1]))).is_err());
        assert!(cast_read_value(&AttributeCast::Float, Some(json!("1e9999"))).is_err());
        assert_eq!(
            cast_read_value(&AttributeCast::Hashed, Some(json!("already-encoded")))?,
            Some(json!("already-encoded"))
        );
        Ok(())
    }

    #[test]
    fn float_cast_recognizes_only_exact_from_float_special_strings() -> Result<()> {
        for text in ["Infinity", "-Infinity", "NaN"] {
            assert!(
                matches!(cast_read_value(&AttributeCast::Float, Some(json!(text))), Err(error)
                if error.code() == "model_attribute_cast_failed" && error.status().as_u16() == 500)
            );
            // 普通PHP(float)仍是0；Model特殊转换不能改变通用scalar的有效golden语义。
            assert_eq!(crate::support::scalars::float(&json!(text)).to_bits(), 0);
        }
        for text in [
            "infinity",
            "INFINITY",
            "INF",
            "nan",
            "NAN",
            "+Infinity",
            " Infinity",
            " infinity",
            "Infinity ",
            "Infinitytail",
            "-Infinitysuffix",
            " NaN ",
            "NaNtail",
            "普通文本",
            "",
        ] {
            let value = cast_read_value(&AttributeCast::Float, Some(json!(text)))?
                .ok_or_else(cast_failed)?;
            assert!(value.as_number().is_some_and(serde_json::Number::is_f64));
            assert_eq!(value.as_f64().ok_or_else(cast_failed)?.to_bits(), 0);
        }
        Ok(())
    }

    #[test]
    fn float_cast_preserves_finite_primitives_null_and_string_conversion_failures() -> Result<()> {
        for (value, bits) in [
            (json!(true), 0x3ff0_0000_0000_0000_u64),
            (json!(false), 0),
            (json!(42), 0x4045_0000_0000_0000),
            (json!(-1.25), 0xbff4_0000_0000_0000),
            (json!("  12.5tail"), 0x4029_0000_0000_0000),
            (json!("7.5tail"), 0x401e_0000_0000_0000),
            (json!("1.25"), 0x3ff4_0000_0000_0000),
            (json!("-0.0"), 0x8000_0000_0000_0000),
        ] {
            let actual =
                cast_read_value(&AttributeCast::Float, Some(value))?.ok_or_else(cast_failed)?;
            assert_eq!(actual.as_f64().ok_or_else(cast_failed)?.to_bits(), bits);
        }
        for value in [
            json!([]),
            json!([1]),
            json!({}),
            json!({"n":1}),
            json!("1e9999"),
            json!("1e400"),
            json!("-1e400"),
        ] {
            assert!(
                matches!(cast_read_value(&AttributeCast::Float, Some(value)), Err(error)
                if error.code() == "model_attribute_cast_failed" && error.status().as_u16() == 500)
            );
        }
        assert_eq!(cast_read_value(&AttributeCast::Float, None)?, None);
        assert_eq!(
            cast_read_value(&AttributeCast::Float, Some(Value::Null))?,
            Some(Value::Null)
        );
        Ok(())
    }

    #[test]
    fn decimal_exponent_limit_ignores_leading_zeros_and_retains_the_original_mantissa() {
        for text in [
            "7.5",
            "1e19998",
            "1e19999",
            "1e-19999",
            "1e+0000000000000000000019999",
            "1E-000000000000000000000000000",
        ] {
            assert!(matches!(model_decimal_text(text), Cow::Borrowed(value) if value == text));
        }
        for (text, expected) in [
            ("1e20000", "1e19999"),
            ("1e+20000", "1e19999"),
            ("1E000000000000000000000020000", "1e19999"),
            ("1e100000000", "1e19999"),
            ("1e4294967296", "1e19999"),
            ("1e-20000", "1e-19999"),
            ("1e-100000000", "1e-19999"),
            ("1e-4294967296", "1e-19999"),
            ("1E-00000000000000000000020000", "1e-19999"),
            ("0.00000001e20000", "0.00000001e19999"),
            (
                "100000000000000000000e-20000",
                "100000000000000000000e-19999",
            ),
        ] {
            assert!(matches!(model_decimal_text(text), Cow::Owned(value) if value == expected));
        }
    }

    #[test]
    fn float_string_prefix_uses_zend_exponent_limit_before_decimal_scale() -> Result<()> {
        let small = format!("0.{}1", "0".repeat(19_999));
        let large = format!("1{}", "0".repeat(20_000));
        for (text, bits) in [
            (format!("{small}e19998"), 0x3f84_7ae1_47ae_147b_u64),
            (format!("{small}e19999"), 0x3fb9_9999_9999_999a),
            (format!("{small}e20000"), 0x3fb9_9999_9999_999a),
            (format!("{small}E+00000000000020000"), 0x3fb9_9999_9999_999a),
            (format!("{small}e4294967296"), 0x3fb9_9999_9999_999a),
            (format!("{large}e-19998"), 0x4059_0000_0000_0000),
            (format!("{large}e-19999"), 0x4024_0000_0000_0000),
            (format!("{large}e-20000"), 0x4024_0000_0000_0000),
            (format!("{large}e-0000000000020000"), 0x4024_0000_0000_0000),
            (format!("{large}e-4294967296"), 0x4024_0000_0000_0000),
            (format!("\t+{small}e20000suffix"), 0x3fb9_9999_9999_999a),
            (format!("  -{large}e-20000tail"), 0xc024_0000_0000_0000),
        ] {
            let value = cast_read_value(&AttributeCast::Float, Some(Value::String(text)))?
                .ok_or_else(cast_failed)?;
            assert_eq!(value.as_f64().ok_or_else(cast_failed)?.to_bits(), bits);
        }
        Ok(())
    }

    #[test]
    fn integer_string_clamp_reuses_existing_saturation_and_nonfinite_rules() -> Result<()> {
        let small = format!("0.{}1e20000", "0".repeat(19_999));
        let large = format!("1{}e-20000", "0".repeat(20_000));
        for (text, expected) in [
            (small, 0),
            (large.clone(), 10),
            (format!("\t+{large}suffix"), 10),
            (format!("-{large}"), -10),
            ("1e400".into(), 0),
            ("-1e400".into(), 0),
            ("1e20000".into(), 0),
            ("NaN".into(), 0),
            ("Infinity".into(), 0),
            ("9223372036854775808".into(), i64::MAX),
            ("-9223372036854775809".into(), i64::MIN),
        ] {
            assert_eq!(
                cast_read_value(&AttributeCast::Integer, Some(Value::String(text)))?,
                Some(Value::from(expected)),
            );
        }
        Ok(())
    }
}
