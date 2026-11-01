//! 模块激活来源；运行时状态文件与模块源码、资源根分别配置。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 已实现的模块激活配置；自定义驱动经编译清单登记，不加载任意类名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    /// 模块源码主路径；相对应用根解析，与持久状态文件独立。
    pub paths: ModulePathsConfig,
    /// 主路径以外的显式扫描位置。
    pub scan: ModuleScanConfig,
    /// 从清单中选择的激活器名称。
    pub activator: String,
    /// 各激活器的部署参数。
    pub activators: ModuleActivatorsConfig,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            paths: ModulePathsConfig::default(),
            scan: ModuleScanConfig::default(),
            activator: "file".to_owned(),
            activators: ModuleActivatorsConfig::default(),
        }
    }
}

/// 模块资源路径；Rust 项目使用已获准的 `modules/` 大小写约定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulePathsConfig {
    /// 主模块目录，默认 `modules`。
    pub modules: PathBuf,
}

impl Default for ModulePathsConfig {
    fn default() -> Self {
        Self {
            modules: PathBuf::from("modules"),
        }
    }
}

/// 额外模块扫描；保留路径顺序和重复项，不等同运行未编译源码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleScanConfig {
    /// 默认关闭额外扫描，主模块目录始终参与发现。
    pub enabled: bool,
    /// 启用后追加的位置，默认 `vendor/*/*`，每个位置继续匹配直接子模块。
    pub paths: Vec<PathBuf>,
}

impl Default for ModuleScanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            paths: vec![PathBuf::from("vendor/*/*")],
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
