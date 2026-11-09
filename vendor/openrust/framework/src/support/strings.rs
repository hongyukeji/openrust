//! 使用固定 portable-ascii 数据完成默认 ASCII 转写。
//!
//! 数据来自portable-ascii 2.0.2固定commit；来源哈希和MIT许可随数据保留。
//! 输入是Rust有效UTF-8 `&str`，因此不接受PHP字符串可包含的非法字节；没有
//! 用有损替换伪造这类输入。这里只实现官方默认strict=false，不伪造ICU严格模式。

use crate::{Error, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

/// 字符串转写工具；当前支持固定的默认 ASCII 映射。
pub struct Str;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransliterationData {
    banks: BTreeMap<u32, Vec<Option<String>>>,
    whitespace: Vec<(String, String)>,
    msword: Vec<(String, String)>,
    bidi: Vec<u32>,
}
static DATA: OnceLock<std::result::Result<TransliterationData, String>> = OnceLock::new();
fn data() -> Result<&'static TransliterationData> {
    match DATA.get_or_init(|| {
        serde_json::from_str(include_str!("strings/portable-ascii-2.0.2.json"))
            .map_err(|error| error.to_string())
    }) {
        Ok(data) => Ok(data),
        Err(_) => Err(Error::custom_with_status(
            crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "transliteration_data_invalid",
            "固定转写数据无法读取",
        )),
    }
}

impl Str {
    /// 按固定`Str::transliterate(value, "?", false)`转写，不修剪或再次转小写。
    pub fn transliterate(value: &str) -> Result<String> {
        Self::transliterate_with_unknown(value, Some("?"))
    }

    /// 覆写未知字符替代文本；None保留未知字符及映射为空的原字符。
    ///
    /// 空字符串表示删除未知字符。保留官方ASCII快路径、Unicode清理与PCRE
    /// dotall匹配语义；不能将此过程近似为无条件逐字符转写或Unicode规范化。
    pub fn transliterate_with_unknown(value: &str, unknown: Option<&str>) -> Result<String> {
        if ascii_fast_path(value) {
            return Ok(value.to_owned());
        }
        let data = data()?;
        let cleaned = clean(value, data);
        if cleaned != value && ascii_fast_path(&cleaned) {
            return Ok(cleaned);
        }
        let mut output = String::new();
        output.try_reserve(cleaned.len()).map_err(Error::internal)?;
        for character in cleaned.chars() {
            // 固定PHP /.|[^\x00]$/us 的s为DOTALL；有效UTF-8逐scalar匹配，LF同样保留。
            if character.is_ascii() {
                output
                    .try_reserve(character.len_utf8())
                    .map_err(Error::internal)?;
                output.push(character);
                continue;
            }
            let codepoint = u32::from(character);
            let mapped = data
                .banks
                .get(&(codepoint >> 8))
                .and_then(|bank| bank.get((codepoint & 255) as usize))
                .and_then(Option::as_deref);
            let replacement = match mapped {
                Some("") if unknown.is_none() => None,
                Some("[?]" | "[?] ") | None => unknown,
                value => value,
            };
            if let Some(text) = replacement {
                output.try_reserve(text.len()).map_err(Error::internal)?;
                output.push_str(text);
            } else {
                output
                    .try_reserve(character.len_utf8())
                    .map_err(Error::internal)?;
                output.push(character);
            }
        }
        Ok(output)
    }
}

fn ascii_fast_path(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, 0x09 | 0x10 | 0x13 | 0x0a | 0x0d | 0x20..=0x7e))
}
fn clean(value: &str, data: &TransliterationData) -> String {
    // 有效UTF-8已由&str保证；对应PHP clean的非法字节移除阶段无需有损解码。
    let mut cleaned: String = value
        .chars()
        .filter(|character| !data.bidi.contains(&u32::from(*character)))
        .collect();
    for (search, replacement) in &data.whitespace {
        cleaned = cleaned.replace(search, replacement);
    }
    for (search, replacement) in &data.msword {
        cleaned = cleaned.replace(search, replacement);
    }
    cleaned.retain(|character| !matches!(character, '\u{0000}'..='\u{0008}' | '\u{000b}'..='\u{000c}' | '\u{000e}'..='\u{001f}' | '\u{007f}'));
    cleaned
}

#[cfg(test)]
mod tests;
