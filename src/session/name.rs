//! 仅为 Session 默认 Cookie 名推导固定 Laravel slug，不公开通用转写 API。
//!
//! Laravel framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181：
//! `Support/Str.php` 142–144、1661–1683，`config/session.php`。
//! portable-ascii 2.0.2（commit 16c17671a804bb92602822113dd91fbc8a35d2af）
//! 是本簇显式固定的参考依赖；Laravel 仅声明 ^2.0.2，并未锁定该解析版本。
//!
//! 固定来源：<https://github.com/voku/portable-ascii/tree/2.0.2>。
//! `ASCII.php` SHA256：1df54d7968ecd765245a1afa58f4a283d5ffa34fe85157f1b247340f57d3b972；
//! `data/ascii_by_languages.php`：e089edb1b32df867bf27f3bca224228f49a850a75bfa4f693d293306e83db72c；
//! `data/ascii_language_max_key.php`：ba2c6818014dcce00ba3965203dd1585889f62fec342ed3133527257b81c1448。
//! 转写数据及派生算法遵守同目录 `name/LICENSE-portable-ascii.txt` 的 MIT 许可。
//! Laravel 派生规则的完整许可与版权见 `name/LICENSE-Laravel.txt`。
//!
//! 数据再生成：将上述官方源码放在同一目录并保持 `ASCII.php`、`data/` 布局，
//! 在其目录执行以下 PHP（JSON 标志保证 UTF-8 与换行一致）：
//! ```text
//! require 'ASCII.php';
//! $m = array_merge(voku\helper\ASCII::charsArrayWithSingleLanguageValues(false, false),
//!     voku\helper\ASCII::charsArrayWithOneLanguage('en', false, false));
//! $m = array_filter($m, fn($key) => preg_match_all('/./us', $key) <= 3, ARRAY_FILTER_USE_KEY);
//! echo json_encode($m, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES
//!     | JSON_UNESCAPED_LINE_TERMINATORS), "\n";
//! ```
//! 1468 项数据 SHA256：21e4514e39ded310a4ff55700a4a143e2ea77b6e82cd6547b49daac746d84899。
//! 默认 en 的最大组合开关为 0，源码仍无条件处理三、二、单字符；四、五字符不可达。
//! 所有映射结果均为 ASCII，因此随后的 ASCII 小写与固定 `Str::lower` 结果相同。

use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use crate::{Error, Result};

const MAX_BYTES: usize = 20 * 1024 * 1024;
type Mapping = HashMap<String, String>;
static MAPPING: OnceLock<std::result::Result<Mapping, ()>> = OnceLock::new();

/// 推导 `Str::slug(app.name, '_') + '_session'`；空 slug 仍得到 `_session`。
///
/// 每步字符串增长受 20 MiB 配置工作预算约束；最终 Cookie 头由配置装配统一验证。
pub(super) fn default_cookie_name(app_name: &str) -> Result<String> {
    let ascii = portable_ascii(app_name)?;
    let mut slug = String::new();
    let mut separator = false;
    for byte in ascii.bytes() {
        match byte {
            b'@' => {
                bounded(
                    slug.len()
                        .checked_add(if slug.is_empty() { 2 } else { 3 })
                        .ok_or_else(invalid)?,
                )?;
                if !slug.is_empty() {
                    slug.push('_');
                }
                slug.push_str("at");
                separator = true;
            }
            b'-' | b'_' | b' ' => separator = !slug.is_empty(),
            byte if byte.is_ascii_alphanumeric() => {
                bounded(
                    slug.len()
                        .checked_add(if separator { 2 } else { 1 })
                        .ok_or_else(invalid)?,
                )?;
                if separator {
                    slug.push('_');
                }
                slug.push(char::from(byte.to_ascii_lowercase()));
                separator = false;
            }
            _ => {}
        }
        bounded(slug.len())?;
    }
    bounded(
        slug.len()
            .checked_add("_session".len())
            .ok_or_else(invalid)?,
    )?;
    slug.push_str("_session");
    Ok(slug)
}

fn mapping() -> Result<&'static Mapping> {
    MAPPING
        .get_or_init(|| {
            let map: Mapping =
                serde_json::from_str(include_str!("name/ascii-en.json")).map_err(|_| ())?;
            if map.values().any(|value| !value.is_ascii()) {
                return Err(());
            }
            Ok(map)
        })
        .as_ref()
        .map_err(|()| invalid())
}

fn portable_ascii(input: &str) -> Result<String> {
    bounded(input.len())?;
    let map = mapping()?;
    // 源 preg_match_all 的列表在替换前固定；不能改为贪婪扫描或逐表项循环。
    let matched = input
        .chars()
        .filter(|value| !retained_ascii(*value))
        .collect::<Vec<_>>();
    let mut result = input.to_owned();
    let mut done = HashSet::new();
    let mut absent = HashSet::new();
    for length in [3, 2, 1] {
        for chars in matched.windows(length) {
            let key = chars.iter().collect::<String>();
            let Some(replacement) = map.get(&key) else {
                continue;
            };
            if done.contains(&key) || absent.contains(&key) {
                continue;
            }
            if !result.contains(&key) {
                // 未变化的串无需重复全串搜索；删除映射可能使新组合相邻，故替换后清空。
                absent.insert(key);
                continue;
            }
            // str_replace 使用非重叠全局匹配；先算增长，避免生成超预算中间串。
            let count = result.matches(&key).count();
            let removed = count.checked_mul(key.len()).ok_or_else(invalid)?;
            let added = count.checked_mul(replacement.len()).ok_or_else(invalid)?;
            let size = result
                .len()
                .checked_sub(removed)
                .and_then(|size| size.checked_add(added))
                .ok_or_else(invalid)?;
            bounded(size)?;
            result = result.replace(&key, replacement);
            done.insert(key);
            absent.clear();
        }
    }
    // 与源先换行归一、再删除不支持字符的顺序一致；LF+CR 是一个替换项。
    result = result.replace("\n\r", " ").replace(['\n', '\r', '\t'], " ");
    result.retain(retained_ascii);
    Ok(result)
}

fn retained_ascii(value: char) -> bool {
    matches!(
        value,
        '\u{09}' | '\u{10}' | '\u{13}' | '\n' | '\r' | ' '..='~'
    )
}

fn bounded(bytes: usize) -> Result<()> {
    if bytes > MAX_BYTES {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn invalid() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_config_invalid",
        "Session Cookie 名称配置无效",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // PHP8.5.0 -n 实跑官方 ASCII::to_ascii；后接固定 slug 正则及 ASCII strtolower。
    // Str::lower 未直接运行（该 PHP 无 mbstring），其输入已由 oracle 验为纯 ASCII。
    #[test]
    fn fixed_portable_ascii_oracle_and_slug_rules_match() -> Result<()> {
        for (input, expected) in [
            ("Laravel", "laravel_session"),
            ("My App", "my_app_session"),
            ("  My--App___Name  ", "my_app_name_session"),
            ("Café déjà vu", "cafe_deja_vu_session"),
            ("ÄÖÜ Straße", "aou_strasse_session"),
            ("Αυτοκίνητο ΕΥ Εύ Ου", "autokinito_ef_eu_oy_session"),
            ("Москва приложение", "moskva_prilozenie_session"),
            ("中文 应用", "_session"),
            ("日本語", "_session"),
            ("مرحبا تطبيق", "mrhba_ttbyk_session"),
            ("Crème@Example.com", "creme_at_examplecom_session"),
            ("foo.bar", "foobar_session"),
            ("---___", "_session"),
            ("", "_session"),
            ("A\u{00a0}B\u{2003}C", "a_b_c_session"),
            ("Cafe\u{0301}", "cafe_session"),
            ("foo@@bar", "foo_at_at_bar_session"),
            (" A\tB\n\rC\u{10}D\u{13}E\u{0b}F ", "a_b_cdef_session"),
            ("γγ γκ γξ γχ", "ghgh_ghk_ghks_ghkh_session"),
            ("কর্ম বাংলাদেশ", "krm_banglades_session"),
            ("¼ ½ ¾", "_session"),
            ("a_b-c", "a_b_c_session"),
            ("0", "0_session"),
        ] {
            let name = default_cookie_name(input)?;
            assert_eq!(name, expected);
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            );
        }
        Ok(())
    }

    #[test]
    fn mapping_and_each_string_growth_are_bounded_without_ascii_fallback() -> Result<()> {
        assert_eq!(mapping()?.len(), 1468);
        assert!(bounded(MAX_BYTES).is_ok());
        let error = bounded(MAX_BYTES + 1).err().ok_or_else(invalid)?;
        assert_eq!(error.code(), "session_config_invalid");
        assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        let source = "a".repeat(MAX_BYTES + 1);
        assert_eq!(
            default_cookie_name(&source)
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_config_invalid".to_owned())
        );
        // 输入本身合法，但加固定后缀会越过输出预算。
        let source = "a".repeat(MAX_BYTES);
        assert_eq!(
            default_cookie_name(&source)
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_config_invalid".to_owned())
        );
        Ok(())
    }
}
