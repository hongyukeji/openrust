use openrust::contracts::config::Repository;
use openrust_settings::{SettingsError, SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

/// 应用示例：页面通过显式投影读取网站信息，设置底座不注册后台路由。
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SiteSettings {
    pub name: String,
    pub title: String,
    pub description: String,
    pub keywords: Vec<String>,
}

impl SettingsGroup for SiteSettings {
    fn group() -> &'static str {
        "site"
    }

    fn defaults(config: &dyn Repository) -> SettingsResult<Self> {
        let name = config
            .get("app.name")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("OpenRust".into());
        Ok(Self {
            title: name.clone(),
            name,
            description: String::new(),
            keywords: Vec::new(),
        })
    }

    fn validate(&self) -> SettingsResult<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 255
            || self.title.len() > 255
            || self.description.len() > 4096
            || self.keywords.len() > 32
            || self.keywords.iter().any(|key| key.len() > 255)
        {
            return Err(SettingsError::InvalidValue);
        }
        Ok(())
    }
}
