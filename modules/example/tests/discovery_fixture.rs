//! 临时部署只复制声明和文件占位；实际初始化始终调用已编译实现。

use openrust::{Error, Result};
use std::path::Path;

pub(crate) fn write(root: &Path) -> Result<()> {
    let module = root.join("modules/example");
    std::fs::create_dir_all(&module).map_err(Error::internal)?;
    std::fs::write(module.join("module.json"), include_str!("../module.json"))
        .map_err(Error::internal)?;
    std::fs::write(
        module.join("module_init.rs"),
        "// 临时部署占位；运行期不编译此 Rust 文件。\n",
    )
    .map_err(Error::internal)?;
    Ok(())
}
