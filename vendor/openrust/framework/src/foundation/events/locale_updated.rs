use crate::contracts::events::Event;

/// 应用语言已切换；监听器执行时配置和译器均已更新。
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
#[derive(Clone, Debug)]
pub struct LocaleUpdated {
    pub locale: String,
    pub previousLocale: Option<String>,
}

impl LocaleUpdated {
    pub const NAME: &'static str = "foundation.locale_updated";
}

impl Event for LocaleUpdated {
    fn name(&self) -> &str {
        Self::NAME
    }
}
