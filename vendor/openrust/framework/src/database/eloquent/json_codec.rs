//! Eloquent 属性 JSON 的进程级编解码扩展。

use super::{attribute_cast::decode_default_json, json_encoding};
use crate::Result;
use serde_json::Value;
use std::sync::{Arc, PoisonError, RwLock};

/// Eloquent JSON 编码器。接收原始 JSON 值和编码 flags。
pub type JsonEncoder = dyn Fn(&Value, u32) -> Result<Value> + Send + Sync;
/// Eloquent JSON 解码器。接收原始值和可选的关联数组标记。
pub type JsonDecoder = dyn Fn(&Value, Option<bool>) -> Result<Value> + Send + Sync;

static ENCODER: RwLock<Option<Arc<JsonEncoder>>> = RwLock::new(None);
static DECODER: RwLock<Option<Arc<JsonDecoder>>> = RwLock::new(None);

/// Eloquent casts 共用的 JSON 编解码入口。
pub struct Json;

impl Json {
    /// 将尖括号编码为十六进制转义。
    pub const HEX_TAG: u32 = 1;
    /// 将与号编码为十六进制转义。
    pub const HEX_AMP: u32 = 2;
    /// 将单引号编码为十六进制转义。
    pub const HEX_APOS: u32 = 4;
    /// 将双引号编码为十六进制转义。
    pub const HEX_QUOT: u32 = 8;
    /// 递归将列表编码为对象。
    pub const FORCE_OBJECT: u32 = 16;
    /// 将完整 PHP 数值字符串编码为数字，保留字符串键。
    pub const NUMERIC_CHECK: u32 = 32;
    /// 直接输出斜线。
    pub const UNESCAPED_SLASHES: u32 = 64;
    /// 使用四空格缩进及换行。
    pub const PRETTY_PRINT: u32 = 128;
    /// 对应 PHP 的 `JSON_UNESCAPED_UNICODE`。
    pub const UNESCAPED_UNICODE: u32 = 256;
    /// 编码遇错时保留可输出结果，优先于 `THROW_ON_ERROR`。
    pub const PARTIAL_OUTPUT_ON_ERROR: u32 = 512;
    /// 保留浮点数的小数类型。
    pub const PRESERVE_ZERO_FRACTION: u32 = 1024;
    /// 与 `UNESCAPED_UNICODE` 同用时直接输出 Unicode 行分隔符。
    pub const UNESCAPED_LINE_TERMINATORS: u32 = 2048;
    /// PHP 的无效 UTF-8 忽略标志。Rust JSON 字符串已经过 UTF-8 校验。
    pub const INVALID_UTF8_IGNORE: u32 = 1 << 20;
    /// PHP 的无效 UTF-8 替换标志。Rust JSON 字符串已经过 UTF-8 校验。
    pub const INVALID_UTF8_SUBSTITUTE: u32 = 1 << 21;
    /// 编码失败时返回错误；默认返回布尔 false。
    pub const THROW_ON_ERROR: u32 = 1 << 22;

    /// 使用当前 encoder 或默认实现编码。自定义结果直接返回给调用方。
    pub fn encode(value: &Value, flags: u32) -> Result<Value> {
        let encoder = ENCODER
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(encoder) = encoder {
            encoder(value, flags)
        } else {
            Self::encode_default(value, flags)
        }
    }

    /// 使用当前 decoder 或默认实现解码。关联数组标记原样传给回调。
    pub fn decode(value: &Value, associative: Option<bool>) -> Result<Value> {
        if let Some(decoder) = Self::decoder() {
            decoder(value, associative)
        } else {
            Self::decode_default(value, associative)
        }
    }

    /// 直接使用默认 encoder，应用 PHP 编码 flags 与默认 512 层深度。
    pub fn encode_default(value: &Value, flags: u32) -> Result<Value> {
        json_encoding::encode(value, flags)
    }

    /// 直接使用默认 decoder。关联标记为 `None` 时采用对象解码。
    pub fn decode_default(value: &Value, associative: Option<bool>) -> Result<Value> {
        decode_default_json(value, associative.unwrap_or(false))
    }

    /// 替换全局 encoder。传入 `None` 恢复默认实现。
    pub fn encode_using(encoder: Option<Arc<JsonEncoder>>) {
        let previous = std::mem::replace(
            &mut *ENCODER.write().unwrap_or_else(PoisonError::into_inner),
            encoder,
        );
        drop(previous);
    }

    /// 替换全局 decoder。传入 `None` 恢复默认实现。
    pub fn decode_using(decoder: Option<Arc<JsonDecoder>>) {
        let previous = std::mem::replace(
            &mut *DECODER.write().unwrap_or_else(PoisonError::into_inner),
            decoder,
        );
        drop(previous);
    }

    pub(super) fn decoder() -> Option<Arc<JsonDecoder>> {
        DECODER
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
