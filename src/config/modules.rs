//! 模块激活来源；运行时状态文件与模块源码、资源根分别配置。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 已实现的模块激活配置；自定义驱动经编译清单登记，不加载任意类名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    /// 从清单中选择的激活器名称。
    pub activator: String,
    /// 各激活器的部署参数。
    pub activators: ModuleActivatorsConfig,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            activator: "file".to_owned(),
            activators: ModuleActivatorsConfig::default(),
        }
    }
}

/// 文件激活器的公开参数表。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleActivatorsConfig {
    /// 文件状态来源参数。
    pub file: FileModuleActivatorConfig,
}

/// 文件激活器路径；相对路径以应用根解析，不改变模块资源根。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileModuleActivatorConfig {
    /// 对应固定模块包的 `statuses-file`；可选绝对路径。
    #[serde(rename = "statuses-file")]
    pub statuses_file: PathBuf,
}

impl Default for FileModuleActivatorConfig {
    fn default() -> Self {
        Self {
            statuses_file: PathBuf::from("modules_statuses.json"),
        }
    }
}
