//! 控制器相对命名空间、保守模块登记与可补偿文件提交。

use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, bail};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct FileChange {
    path: PathBuf,
    before: Option<String>,
    after: String,
}

struct FileInput {
    path: PathBuf,
    before: Option<String>,
}

struct Plan {
    root: PathBuf,
    directories: Vec<PathBuf>,
    inputs: Vec<FileInput>,
    files: Vec<FileChange>,
    type_name: String,
}

pub(super) fn generate(name: &str, project: &Path) -> anyhow::Result<String> {
    let plan = plan(name, project)?;
    apply(&plan, &mut |_, _| Ok(()))?;
    Ok(plan.type_name)
}

fn module_name(name: &str, suffix: &str) -> anyhow::Result<String> {
    let module = super::normalize_generator_type(name, suffix)?;
    if matches!(
        module.as_str(),
        "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    ) {
        bail!("`{name}` resolves to a reserved Rust identifier");
    }
    Ok(module)
}

fn path_exists(path: &Path, directory: bool) -> anyhow::Result<bool> {
    match path.symlink_metadata() {
        Ok(metadata) if directory && metadata.is_dir() => Ok(true),
        Ok(metadata) if !directory && metadata.is_file() => Ok(true),
        Ok(_) => bail!(
            "controller path has an unsupported type: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn plan(name: &str, project: &Path) -> anyhow::Result<Plan> {
    let (namespace, class) = name.rsplit_once(['/', '\\']).unwrap_or(("", name));
    if namespace.is_empty() && name != class {
        bail!("controller name must be relative to app/http/controllers");
    }
    let leaf = module_name(class, "Controller")?;
    let type_name = format!("{}Controller", super::pascal_case(&leaf));
    let mut directory = project.to_path_buf();
    for component in ["app", "http", "controllers"] {
        directory.push(component);
        if !path_exists(&directory, true)? {
            bail!(
                "controller directory does not exist: {}",
                directory.display()
            );
        }
    }
    let root = directory.clone();
    let mut modules = if namespace.is_empty() {
        Vec::new()
    } else {
        namespace
            .split(['/', '\\'])
            .map(|part| module_name(part, ""))
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    modules.push(leaf);
    let mut directories = Vec::new();
    let mut inputs = Vec::new();
    let mut files = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        let leaf = index + 1 == modules.len();
        let child = directory.join(module);
        let sibling = directory.join(format!("{module}.rs"));
        if path_exists(&sibling, false)? || (path_exists(&child, true)? && leaf) {
            bail!("controller module already exists: {}", sibling.display());
        }
        let registry = directory.join("mod.rs");
        let before = path_exists(&registry, false)?
            .then(|| fs::read_to_string(&registry))
            .transpose()?;
        // 可复用且不写入的父声明也决定新文件是否真实可达。
        inputs.push(FileInput {
            path: registry.clone(),
            before: before.clone(),
        });
        let contents = before.as_deref().unwrap_or("//! 应用控制器。\n");
        if let Some(after) = register(contents, module, !leaf)? {
            files.push(FileChange {
                path: registry,
                before,
                after,
            });
        }
        if leaf {
            inputs.push(FileInput {
                path: sibling.clone(),
                before: None,
            });
            files.push(FileChange {
                path: sibling,
                before: None,
                after: format!("//! 应用控制器。\n\n/// `{type_name}` 控制器；在 `routes/web.rs` 或 `routes/api.rs` 中注册其处理方法。\npub struct {type_name};\n"),
            });
        } else {
            if !path_exists(&child, true)? {
                directories.push(child.clone());
            }
            directory = child;
        }
    }
    Ok(Plan {
        root,
        directories,
        inputs,
        files,
        type_name,
    })
}

struct ModuleItem<'a> {
    name: &'a str,
    start: usize,
    end: usize,
    reusable: bool,
}

fn single_attribute(text: &str) -> bool {
    if text.contains("r#") || text.contains("r\"") {
        return false;
    }
    let mut brackets = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else {
            match character {
                '"' => quoted = true,
                '[' => brackets += 1,
                ']' if brackets != 0 => {
                    brackets -= 1;
                    if brackets == 0 {
                        return text[index + 1..].trim().is_empty();
                    }
                }
                // 字符字面量和注释中的括号需完整 lexer；此处保守拒绝。
                '\'' | '/' => return false,
                _ => {}
            }
        }
    }
    false
}

// 只接受可明确保全的逐行登记语法；不猜测任意 Rust 文件的 AST。
fn registry_items(contents: &str) -> anyhow::Result<Vec<ModuleItem<'_>>> {
    let mut items = Vec::new();
    let mut offset = 0;
    let mut start = 0;
    let mut attached = false;
    for line in contents.split_inclusive('\n') {
        let end = offset + line.len();
        let text = line.trim();
        if text.is_empty() || text.starts_with("//") {
            if text.starts_with("//!") {
                if !items.is_empty() || attached {
                    bail!("unsupported inner controller module documentation");
                }
                start = end;
            } else if text.starts_with("///") {
                attached = true;
            }
        } else if text.starts_with("#[") && single_attribute(text) {
            // 多行属性、宏和块布局不在本有限自动登记语法中。
            attached = true;
        } else if text.starts_with("#![") && single_attribute(text) && items.is_empty() && !attached
        {
            start = end;
        } else {
            let declaration = text
                .split_once("//")
                .map_or(text, |(code, _)| code.trim_end());
            let (rest, public) = if let Some(rest) = declaration.strip_prefix("pub mod ") {
                (rest, true)
            } else if let Some(rest) = declaration.strip_prefix("mod ") {
                (rest, false)
            } else if let Some(rest) = declaration.strip_prefix("pub(") {
                let (_, rest) = rest
                    .split_once(')')
                    .context("unsupported controller module visibility")?;
                (
                    rest.trim_start()
                        .strip_prefix("mod ")
                        .context("unsupported controller registry item")?,
                    false,
                )
            } else {
                bail!("unsupported controller registry item; no files were changed");
            };
            let raw = rest
                .strip_suffix(';')
                .context("controller registry requires external module declarations")?
                .trim();
            let name = raw.strip_prefix("r#").unwrap_or(raw);
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || name.as_bytes()[0].is_ascii_digit()
            {
                bail!("unsupported controller module identifier");
            }
            if items.iter().any(|item: &ModuleItem<'_>| item.name == name) {
                bail!("duplicate controller module declaration: {name}");
            }
            items.push(ModuleItem {
                name,
                start,
                end,
                reusable: public && !attached && raw == name,
            });
            start = end;
            attached = false;
        }
        offset = end;
    }
    if attached {
        bail!("controller registry has an unattached attribute or documentation");
    }
    Ok(items)
}

fn register(contents: &str, module: &str, namespace: bool) -> anyhow::Result<Option<String>> {
    let items = registry_items(contents)?;
    if let Some(item) = items.iter().find(|item| item.name == module) {
        if namespace && item.reusable {
            return Ok(None);
        }
        bail!("controller module is already declared or has unsupported attributes: {module}");
    }
    let insertion = items
        .iter()
        .find(|item| item.name > module)
        .map(|item| item.start)
        .or_else(|| items.last().map(|item| item.end))
        .unwrap_or(contents.len());
    let mut result = String::with_capacity(contents.len() + module.len() + 12);
    result.push_str(&contents[..insertion]);
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    let _ = writeln!(result, "pub mod {module};");
    result.push_str(&contents[insertion..]);
    Ok(Some(result))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    CreateDirectory,
    Stage,
    Backup,
    Install,
    Installed,
    Rollback,
    Cleanup,
}

type Probe<'a> = dyn FnMut(Step, &Path) -> anyhow::Result<()> + 'a;

struct Prepared {
    target: PathBuf,
    stage: Option<PathBuf>,
    backup: Option<PathBuf>,
    installed: bool,
}

#[derive(Default)]
struct Transaction {
    files: Vec<Prepared>,
    directories: Vec<PathBuf>,
}

fn temporary(target: &Path, kind: &str) -> anyhow::Result<(PathBuf, fs::File)> {
    let parent = target.parent().context("controller target has no parent")?;
    loop {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".openrust-controller-{}-{sequence}.{kind}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

impl Transaction {
    fn prepare(&mut self, plan: &Plan, probe: &mut Probe<'_>) -> anyhow::Result<()> {
        for directory in &plan.directories {
            probe(Step::CreateDirectory, directory)?;
            fs::create_dir(directory)?;
            self.directories.push(directory.clone());
        }
        for change in &plan.files {
            let (path, mut file) = temporary(&change.path, "stage")?;
            self.files.push(Prepared {
                target: change.path.clone(),
                stage: Some(path.clone()),
                backup: None,
                installed: false,
            });
            probe(Step::Stage, &path)?;
            file.write_all(change.after.as_bytes())?;
            if change.before.is_some() {
                file.set_permissions(fs::metadata(&change.path)?.permissions())?;
            }
            file.sync_all()?;
            if let Some(before) = &change.before {
                let (path, mut backup) = temporary(&change.path, "backup")?;
                self.files
                    .last_mut()
                    .context("missing prepared controller file")?
                    .backup = Some(path.clone());
                probe(Step::Backup, &path)?;
                backup.write_all(before.as_bytes())?;
                backup.set_permissions(fs::metadata(&change.path)?.permissions())?;
                backup.sync_all()?;
            }
        }
        Ok(())
    }

    fn install(&mut self, probe: &mut Probe<'_>) -> anyhow::Result<()> {
        // 先装新文件，再更新上层登记；失败仍由同一 journal 逆序恢复。
        for item in self.files.iter_mut().rev() {
            probe(Step::Install, &item.target)?;
            let stage = item.stage.as_ref().context("missing controller stage")?;
            if item.backup.is_some() {
                fs::rename(stage, &item.target)?;
                item.stage = None;
            } else {
                // hard_link 不覆盖已有目标，且只发布已写完的同目录文件。
                fs::hard_link(stage, &item.target)?;
            }
            item.installed = true;
            probe(Step::Installed, &item.target)?;
        }
        Ok(())
    }

    fn cleanup(&mut self, rollback: bool, probe: &mut Probe<'_>) -> Vec<String> {
        let mut errors = Vec::new();
        // install 按逆序执行，因此恢复按原序执行。
        for item in &mut self.files {
            if rollback && item.installed {
                let restored = probe(Step::Rollback, &item.target).and_then(|()| {
                    if let Some(backup) = &item.backup {
                        fs::rename(backup, &item.target)?;
                    } else {
                        fs::remove_file(&item.target)?;
                    }
                    Ok(())
                });
                if let Err(error) = restored {
                    errors.push(format!(
                        "rollback {}: {error:#}; backup={:?}",
                        item.target.display(),
                        item.backup
                    ));
                    continue; // 保留恢复文件，不删证据或猜测覆盖目标。
                }
                item.backup = None;
                item.installed = false;
            }
            for path in [&mut item.stage, &mut item.backup] {
                if let Some(file) = path.as_ref() {
                    let result = probe(Step::Cleanup, file)
                        .and_then(|()| fs::remove_file(file).map_err(Into::into));
                    if let Err(error) = result {
                        errors.push(format!("cleanup {}: {error:#}", file.display()));
                    } else {
                        *path = None;
                    }
                }
            }
        }
        if rollback {
            for directory in self.directories.iter().rev() {
                if let Err(error) = fs::remove_dir(directory) {
                    errors.push(format!("cleanup {}: {error}", directory.display()));
                }
            }
        }
        errors
    }
}

fn recheck(plan: &Plan) -> anyhow::Result<()> {
    for input in &plan.inputs {
        let current = path_exists(&input.path, false)?
            .then(|| fs::read_to_string(&input.path))
            .transpose()?;
        if current != input.before {
            bail!(
                "controller input changed before commit: {}",
                input.path.display()
            );
        }
    }
    for directory in &plan.directories {
        if path_exists(directory, true)? {
            bail!(
                "controller directory changed before commit: {}",
                directory.display()
            );
        }
    }
    Ok(())
}

fn apply(plan: &Plan, probe: &mut Probe<'_>) -> anyhow::Result<()> {
    let lock_path = plan.root.join(".openrust-controller.lock");
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .context("controller generation already active or lock cannot be created")?;
    let mut transaction = Transaction::default();
    let result = (|| {
        recheck(plan)?;
        transaction.prepare(plan, probe)?;
        transaction.install(probe)
    })();
    let mut secondary = transaction.cleanup(result.is_err(), probe);
    drop(lock);
    if let Err(error) = fs::remove_file(&lock_path) {
        secondary.push(format!("unlock {}: {error}", lock_path.display()));
    }
    match result {
        Err(primary) if secondary.is_empty() => {
            Err(primary.context("controller_generation_failed: 已恢复生成前文件"))
        }
        Err(primary) => Err(primary.context(format!(
            "controller_generation_failed; controller_cleanup_failed: {}",
            secondary.join("; ")
        ))),
        Ok(()) if secondary.is_empty() => Ok(()),
        Ok(()) => bail!(
            "controller_generated_cleanup_failed: 文件已生成；{}",
            secondary.join("; ")
        ),
    }
}

#[cfg(test)]
#[path = "controller_generation_tests.rs"]
mod tests;
