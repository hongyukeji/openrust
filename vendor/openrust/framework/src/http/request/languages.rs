use std::borrow::Cow;

use crate::translation::{TranslationResult, canonical_locale};

use super::Request;

impl Request {
    /// 按偏好顺序读取标准 Accept-Language；合并重复头，忽略无效标签、非法权重及 q=0。
    /// 标签使用 BCP 47；通配符保留为 `*`，不受 app.locale_header 影响。
    /// 当前实际应用语言请读取 App::current_locale()，此方法不读取或修改应用语言。
    pub fn get_languages(&self) -> &[String] {
        self.languages
            .get_or_init(|| self.parse_language_header(&http::header::ACCEPT_LANGUAGE))
    }

    // 自定义头只在中间件选择时解析，不读取或填充标准 Accept-Language 缓存。
    pub(crate) fn languages_from_header(&self, header: &http::HeaderName) -> Cow<'_, [String]> {
        if header == http::header::ACCEPT_LANGUAGE {
            Cow::Borrowed(self.get_languages())
        } else {
            Cow::Owned(self.parse_language_header(header))
        }
    }

    fn parse_language_header(&self, header: &http::HeaderName) -> Vec<String> {
        let mut entries = Vec::new();
        for header in self.headers().get_all(header) {
            let Ok(header) = header.to_str() else {
                continue;
            };
            for item in header.split(',') {
                if let Some((language, quality)) = preference(item) {
                    entries.push((language, quality));
                }
            }
        }
        // 稳定排序保留同权输入顺序；去重保留同一标签的最高有效权重。
        entries.sort_by(|left, right| right.1.cmp(&left.1));
        let mut languages = Vec::new();
        for (language, _) in entries {
            if !languages.contains(&language) {
                languages.push(language);
            }
        }
        languages
    }

    /// 按标准 Accept-Language 偏好从有序候选中选择语言，无匹配时返回首项。
    /// 不受 app.locale_header 影响，也不代表 App::current_locale() 的最终应用语言。
    /// `None` 或空候选返回请求首选项；非法候选显式报错。
    pub fn get_preferred_language(
        &self,
        locales: Option<&[&str]>,
    ) -> TranslationResult<Option<String>> {
        let preferred = self.get_languages();
        let Some(locales) = locales.filter(|locales| !locales.is_empty()) else {
            return Ok(preferred.first().cloned());
        };
        let locales: Vec<_> = locales
            .iter()
            .map(|locale| canonical_locale(locale))
            .collect::<TranslationResult<_>>()?;
        for language in preferred {
            if language == "*" {
                return Ok(locales.first().cloned());
            }
            for combination in combinations(language) {
                if let Some(locale) = locales.iter().find(|locale| {
                    locale.as_str() == combination
                        || locale
                            .strip_prefix(&combination)
                            .is_some_and(|suffix| suffix.starts_with('-'))
                }) {
                    return Ok(Some(locale.clone()));
                }
            }
        }
        Ok(locales.first().cloned())
    }
}

fn preference(item: &str) -> Option<(String, u16)> {
    let mut segments = item.split(';');
    let language = segments.next()?.trim();
    let mut quality = 1000;
    if let Some(parameter) = segments.next() {
        let (name, value) = parameter.trim().split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("q") || segments.next().is_some() {
            return None;
        }
        quality = qvalue(value.trim())?;
    }
    if quality == 0 {
        return None;
    }
    let language = if language == "*" {
        "*".to_owned()
    } else {
        canonical_locale(language).ok()?
    };
    Some((language, quality))
}

// HTTP qvalue 最多三位小数，用整数比较，避免浮点误差和 NaN。
fn qvalue(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if !matches!(whole, "0" | "1")
        || fraction.len() > 3
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (whole == "1" && fraction.bytes().any(|byte| byte != b'0'))
    {
        return None;
    }
    let mut weight = whole.parse::<u16>().ok()? * 1000;
    for (digit, scale) in fraction.bytes().zip([100, 10, 1]) {
        weight += u16::from(digit - b'0') * scale;
    }
    Some(weight)
}

// 先匹配完整标签，再依次匹配语言／文字／地区的组合，最后匹配基础语言。
// 规范化由现有 ICU 完成；此处不猜测中文地区对应的文字体系。
fn combinations(locale: &str) -> Vec<String> {
    let parsed: icu_locale_core::Locale = locale.parse().expect("请求语言已规范化");
    let id = parsed.id;
    let mut combinations = vec![locale.to_owned(), id.to_string()];
    if let Some(script) = id.script {
        combinations.push(format!("{}-{script}", id.language));
    }
    if let Some(region) = id.region {
        combinations.push(format!("{}-{region}", id.language));
    }
    combinations.push(id.language.to_string());
    combinations.dedup();
    combinations
}
