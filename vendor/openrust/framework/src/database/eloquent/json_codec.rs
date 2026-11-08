//! Eloquent 属性 JSON 的进程级编解码扩展。

use super::attribute_cast::{decode_default_json, encode_default_json};
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
    /// 对应 PHP 的 `JSON_UNESCAPED_UNICODE`。
    pub const UNESCAPED_UNICODE: u32 = 256;

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

    /// 直接使用默认 encoder。内置 flags 支持 0 和 `UNESCAPED_UNICODE`。
    pub fn encode_default(value: &Value, flags: u32) -> Result<Value> {
        encode_default_json(value, flags).map(Value::String)
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
