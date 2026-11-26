//! 在应用构建脚本内生成 Rust 函数；本模块不进入生产执行链。

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

/// 扫描调用方明确选择的模板目录。目录级 Cargo 追踪包含新增及删除文件。
pub fn generate(output: &Path, roots: &[PathBuf]) -> io::Result<String> {
    let directory = output.join("blade-native");
    fs::create_dir_all(&directory)?;
    let mut files = Vec::new();
    for root in roots {
        // 监听最近存在的祖先，既覆盖首次创建，也避免不存在路径让每次 Cargo 都重跑。
        let watch = root.ancestors().find(|path| path.exists()).unwrap_or(root);
        println!("cargo:rerun-if-changed={}", watch.display());
        collect(root, &mut files)?;
    }
    files.sort();
    files.dedup();
    let mut modules = String::new();
    let mut templates = String::new();
    for (index, path) in files.iter().enumerate() {
        let source = fs::read_to_string(path)?;
        let blocks = super::blocks(&source)
            .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))?;
        if blocks.is_empty() {
            continue;
        }
        if source.contains("@__native_block_") {
            return Err(io::Error::other(format!(
                "{}: 原生块占位符为保留语法",
                path.display()
            )));
        }
        let mut entries = String::new();
        for (ordinal, block) in blocks.iter().enumerate() {
            let name = format!("view_{index}_block_{ordinal}");
            syn::parse_str::<syn::Block>(&format!("{{ {} }}", block.body)).map_err(|error| {
                io::Error::other(format!(
                    "{}:{} 原生块 {}: {error}",
                    path.display(),
                    block.line,
                    ordinal + 1
                ))
            })?;
            // 每块独立生成文件，诊断带模板文件名、块号及原始起始行；不伪称精确 span 重映射。
            let filename = format!(
                "{index}-{}-block-{}-line-{}.rs",
                path.file_name().unwrap().to_string_lossy(),
                ordinal + 1,
                block.line
            );
            let generated = directory.join(filename);
            let contents = format!(
                "// 来源 {}:{}\n#[allow(unreachable_code)]\npub(super) fn run(context: &mut ::openrust::view::NativeContext<'_>) -> ::openrust::view::NativeResult {{\nlet _ = &context;\n{}\nOk(())\n}}\n",
                path.display(),
                block.line,
                block.body
            );
            write_changed(&generated, &contents)?;
            writeln!(
                modules,
                "#[path = {:?}] mod {name};",
                generated
                    .to_str()
                    .ok_or_else(|| io::Error::other("模板生成路径必须为 UTF-8"))?
            )
            .unwrap();
            writeln!(
                entries,
                "::openrust::view::NativeBlock::new({:?}, {:?}, {}, {name}::run),",
                block.body,
                path.to_string_lossy(),
                block.line
            )
            .unwrap();
        }
        writeln!(
            templates,
            "::openrust::view::NativeTemplate::new(&[{entries}]),"
        )
        .unwrap();
    }
    Ok(format!(
        "{modules}\npub const NATIVE_VIEWS: &[::openrust::view::NativeTemplate] = &[{templates}];\n"
    ))
}

fn collect(root: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::other(format!(
            "原生模板发现目录不接受符号链接：{}",
            root.display()
        )));
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other(format!(
                "原生模板发现目录不接受符号链接：{}",
                path.display()
            )));
        }
        if kind.is_dir() {
            collect(&path, files)?;
        } else if kind.is_file()
            && path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".blade.html"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn write_changed(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read(path).ok().as_deref() != Some(contents.as_bytes()) {
        fs::write(path, contents)?;
    }
    Ok(())
}
