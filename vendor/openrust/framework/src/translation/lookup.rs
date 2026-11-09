//! 翻译查找钩子。用户代码运行在固定目录快照上，且不占用内部锁。
use super::{CatalogLine, Locale, MessageCatalog, Replacement, Translator};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeSet,
    fmt,
    sync::{Arc, RwLock},
};

/// 缺键处理器收到的原始替换参数，保留调用者的迭代顺序。
#[derive(Clone, Copy)]
pub enum MissingReplacements<'a> {
    /// 普通文本入口的具名参数。
    Text(&'a [(String, String)]),
    /// 混合替换入口的文本、标签和对象值。
    Mixed(&'a [(String, Replacement<'a>)]),
}

/// 未找到翻译时传给应用处理器的查找上下文。
pub struct MissingTranslation<'a> {
    /// 请求的原始键。
    pub key: &'a str,
    /// 查找开始时采用的语言。
    pub locale: &'a Locale,
    /// 此次是否允许查询回退语言。
    pub fallback: bool,
    /// 调用方的替换参数。复数选择在读取资源时传空参数。
    pub replacements: MissingReplacements<'a>,
}

type MissingHandler = dyn Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync;
type LocaleResolver = dyn Fn(&[Locale]) -> Vec<Locale> + Send + Sync;

#[derive(Default)]
pub(super) struct LookupHooks {
    missing: RwLock<Option<Arc<MissingHandler>>>,
    locales: RwLock<Option<Arc<LocaleResolver>>>,
}

impl fmt::Debug for LookupHooks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LookupHooks")
            .finish_non_exhaustive()
    }
}

impl Translator {
    /// 登记或替换缺键处理器。返回 `None` 时保留原键。
    ///
    /// 处理器在查找结束后、替换占位符之前调用。存在性查询跳过此处理器。
    /// 同一译器在处理器中的同步重入也跳过处理器，避免递归触发。
    pub fn handle_missing_keys_using(
        &self,
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.set_missing_handler(Some(Arc::new(handler)));
    }

    /// 清除当前译器的缺键处理器。
    pub fn clear_missing_key_handler(&self) {
        self.set_missing_handler(None);
    }

    /// 登记或替换分组回退顺序。解析器接收起始语言及配置的回退语言。
    ///
    /// 结果保留首次出现顺序。精确 JSON 命中和关闭回退时跳过此解析器。
    pub fn determine_locales_using(
        &self,
        resolver: impl Fn(&[Locale]) -> Vec<Locale> + Send + Sync + 'static,
    ) {
        self.set_locale_resolver(Some(Arc::new(resolver)));
    }

    /// 恢复默认分组回退顺序。
    pub fn clear_locale_resolver(&self) {
        self.set_locale_resolver(None);
    }

    fn set_missing_handler(&self, handler: Option<Arc<MissingHandler>>) {
        let old = std::mem::replace(
            &mut *self
                .lookup_hooks
                .missing
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            handler,
        );
        drop(old);
    }

    fn set_locale_resolver(&self, resolver: Option<Arc<LocaleResolver>>) {
        let old = std::mem::replace(
            &mut *self
                .lookup_hooks
                .locales
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            resolver,
        );
        drop(old);
    }

    pub(super) fn lookup_snapshot(&self) -> Arc<MessageCatalog> {
        self.catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    pub(super) fn lookup_value<'a>(
        &self,
        catalog: &'a MessageCatalog,
        locale: &Locale,
        key: &str,
        fallback: bool,
    ) -> Option<(&'a Locale, CatalogLine<'a>, bool)> {
        if let Some((locale, line)) = catalog.json_line_at(locale, key) {
            return Some((locale, line, true));
        }
        let group = |locale| {
            catalog
                .group_line_at(locale, key, true)
                .map(|(locale, line)| (locale, line, false))
        };
        if !fallback {
            return group(locale);
        }

        let resolver = self
            .lookup_hooks
            .locales
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let fallback = &self.config.fallback_locale;
        if let Some(resolver) = resolver {
            let locales = resolver(&[locale.clone(), fallback.clone()]);
            let mut seen = BTreeSet::new();
            locales
                .iter()
                .filter(|locale| seen.insert(*locale))
                .find_map(group)
        } else {
            group(locale).or_else(|| (locale != fallback).then(|| group(fallback)).flatten())
        }
    }

    pub(super) fn missing_text<'a>(
        &self,
        key: &'a str,
        locale: &Locale,
        fallback: bool,
        replacements: MissingReplacements<'_>,
    ) -> Cow<'a, str> {
        let handler = self
            .lookup_hooks
            .missing
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(handler) = handler
            && let Some(_guard) = self.suppress_missing()
            && let Some(text) = handler(MissingTranslation {
                key,
                locale,
                fallback,
                replacements,
            })
        {
            return Cow::Owned(text);
        }
        Cow::Borrowed(key)
    }

    pub(super) fn suppress_missing(&self) -> Option<MissingGuard> {
        MissingGuard::enter(Arc::as_ptr(&self.lookup_hooks))
    }
}

thread_local! {
    static MISSING_ACTIVE: RefCell<Vec<*const LookupHooks>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct MissingGuard(*const LookupHooks);

impl MissingGuard {
    fn enter(hooks: *const LookupHooks) -> Option<Self> {
        MISSING_ACTIVE.with_borrow_mut(|active| {
            if active.contains(&hooks) {
                return None;
            }
            active.push(hooks);
            Some(Self(hooks))
        })
    }
}

impl Drop for MissingGuard {
    fn drop(&mut self) {
        MISSING_ACTIVE.with_borrow_mut(|active| {
            active.retain(|hooks| *hooks != self.0);
        });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/translation/lookup_hooks.rs"]
mod tests;
