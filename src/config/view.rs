//! 服务端 HTML 视图配置。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// `[view]` 区段：应用模板目录与开发期重载。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    /// 模板目录；相对路径以应用工作目录为基准。
    pub directory: PathBuf,
    /// 每次渲染前检查文件修改时间，适用于本地开发。
    pub auto_reload: bool,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("views"),
            auto_reload: false,
        }
    }
}
