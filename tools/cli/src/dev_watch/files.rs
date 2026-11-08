use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use openrust::config::ConfigLoader;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Change {
    None,
    Source,
    Manifest,
}

pub(super) struct Files {
    app: PathBuf,
    roots: BTreeSet<PathBuf>,
    manifest_roots: BTreeSet<PathBuf>,
    excluded: BTreeSet<PathBuf>,
    namespace: String,
    environment: Option<String>,
    dotenv: PathBuf,
}

impl Files {
    pub(super) fn exclude(&mut self, path: &Path) {
        self.excluded.insert(physical_path(path));
    }

    pub(super) fn new(
        metadata: &Value,
        app: &Path,
        workspace: &Path,
        namespace: &str,
        environment: Option<&str>,
    ) -> anyhow::Result<Self> {
        let app = app.canonicalize().context("无法定位开发应用目录")?;
        let mut roots = BTreeSet::from([app.clone()]);
        let mut manifest_roots = BTreeSet::from([workspace.canonicalize()?]);
        for manifest in local_manifests(metadata, &app)? {
            let root = manifest.parent().context("Cargo 清单没有父目录")?;
            roots.insert(root.to_path_buf());
            if let Some(workspace) = enclosing_workspace(&manifest)? {
                manifest_roots.insert(workspace);
            }
        }

        // 目录名相对各个源码根解释，例如 /tmp 下的整个项目仍可监视。
        let mut excluded = roots
            .iter()
            .flat_map(|root| {
                [
                    ".agents",
                    ".git",
                    "bootstrap/cache",
                    "dist",
                    "docker-data",
                    "node_modules",
                    "storage",
                    "target",
                    "tmp",
                ]
                .map(|name| root.join(name))
            })
            .filter(|path| !roots.iter().any(|root| root.starts_with(path)))
            .collect::<BTreeSet<_>>();
        let target = metadata["target_directory"]
            .as_str()
            .map(Path::new)
            .filter(|path| path.is_absolute())
            .context("Cargo metadata 缺少绝对 target_directory")?;
        let target = physical_path(target);
        if roots.iter().any(|root| root.starts_with(&target)) {
            // 用户可把 CARGO_TARGET_DIR 指向应用的祖先，此时不能丢弃整个源码根。
            excluded.extend(
                ["debug", "release", "doc"]
                    .map(|name| target.join(name))
                    .into_iter()
                    .filter(|path| !roots.iter().any(|root| root.starts_with(path))),
            );
        } else {
            excluded.insert(target);
        }

        let dotenv = ConfigLoader::application_dotenv_path(&app, namespace, environment)?;
        Ok(Self {
            app,
            roots,
            manifest_roots,
            excluded,
            namespace: namespace.to_owned(),
            environment: environment.map(str::to_owned),
            dotenv,
        })
    }

    pub(super) fn watch_args(&self) -> Vec<OsString> {
        let origin = self
            .app
            .ancestors()
            .find(|origin| {
                self.roots
                    .iter()
                    .chain(&self.manifest_roots)
                    .all(|root| root.starts_with(origin))
            })
            .unwrap_or(&self.app);
        let mut args = [
            "--only-emit-events",
            "--emit-events-to=json-stdio",
            "--no-discover-ignore",
            "--no-meta",
            "--no-follow-symlinks",
            "--project-origin",
        ]
        .map(OsString::from)
        .to_vec();
        args.push(origin.as_os_str().to_owned());
        for root in &self.roots {
            if !root
                .ancestors()
                .skip(1)
                .any(|parent| self.roots.contains(parent))
            {
                args.extend([OsString::from("--watch"), root.as_os_str().to_owned()]);
            }
        }
        for root in &self.manifest_roots {
            for directory in [root.clone(), root.join(".cargo")] {
                if directory.is_dir() && !self.roots.iter().any(|root| directory.starts_with(root))
                {
                    args.extend([
                        OsString::from("--watch-non-recursive"),
                        directory.into_os_string(),
                    ]);
                }
            }
        }
        for path in &self.excluded {
            if let Ok(relative) = path.strip_prefix(origin)
                && !relative.as_os_str().is_empty()
                && let Some(pattern) = ignore_pattern(relative)
            {
                args.extend([OsString::from("--ignore"), pattern.into()]);
            }
        }
        args
    }

    pub(super) fn classify(&mut self, line: &str) -> anyhow::Result<Change> {
        let event: Value = serde_json::from_str(line).context("Watchexec 返回无效 JSON 事件")?;
        let tags = event["tags"]
            .as_array()
            .context("Watchexec 事件缺少 tags")?;
        if tags
            .iter()
            .any(|tag| tag["kind"] == "fs" && tag["simple"] == "access")
        {
            return Ok(Change::None);
        }

        let mut change = Change::None;
        for tag in tags.iter().filter(|tag| tag["kind"] == "path") {
            let path = tag["absolute"]
                .as_str()
                .map(Path::new)
                .filter(|path| path.is_absolute())
                .context("Watchexec 路径事件缺少绝对路径")?;
            change = change.max(self.classify_path(path)?);
        }
        Ok(change)
    }

    fn classify_path(&mut self, path: &Path) -> anyhow::Result<Change> {
        if self.excluded.iter().any(|root| path.starts_with(root)) {
            return Ok(Change::None);
        }
        if self.manifest_roots.iter().any(|root| {
            path.strip_prefix(root).is_ok_and(|relative| {
                [
                    "Cargo.toml",
                    "Cargo.lock",
                    ".cargo",
                    ".cargo/config",
                    ".cargo/config.toml",
                ]
                .iter()
                .any(|name| relative == Path::new(name))
            })
        }) {
            return Ok(Change::Manifest);
        }
        let Some(root) = self.roots.iter().rfind(|root| path.starts_with(root)) else {
            return Ok(Change::None);
        };
        let relative = path.strip_prefix(root)?;
        let Some(first) = relative
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str())
        else {
            return Ok(Change::None);
        };
        if root == &self.app && (first == ".env" || first.starts_with(".env.")) {
            let selected = ConfigLoader::application_dotenv_path(
                &self.app,
                &self.namespace,
                self.environment.as_deref(),
            )?;
            let changed = selected != self.dotenv || path == selected;
            self.dotenv = selected;
            return Ok(if changed {
                Change::Source
            } else {
                Change::None
            });
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let extension = path.extension().and_then(|extension| extension.to_str());
        if name.starts_with(".openrust-migration-") && extension == Some("lock")
            || first == "database" && (extension == Some("sqlite") || name.contains(".sqlite-"))
        {
            return Ok(Change::None);
        }
        if matches!(name, "Cargo.toml" | "Cargo.lock") {
            return Ok(Change::Manifest);
        }
        let source = root != &self.app
            || matches!(
                first,
                "app"
                    | "bootstrap"
                    | "config"
                    | "database"
                    | "lang"
                    | "modules"
                    | "proto"
                    | "public"
                    | "resources"
                    | "routes"
                    | "src"
                    | "artisan"
            )
            || matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "toml")
            );
        Ok(if source { Change::Source } else { Change::None })
    }
}

/// 遍历实际应用的解析图，跨注册表节点也能找出被 patch 替换为本地路径的依赖。
fn local_manifests(metadata: &Value, app: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let packages = metadata["packages"]
        .as_array()
        .context("Cargo metadata 缺少 packages")?;
    let mut paths = HashMap::new();
    for package in packages
        .iter()
        .filter(|package| package["source"].is_null())
    {
        let id = package["id"]
            .as_str()
            .context("Cargo metadata 缺少本地包 ID")?;
        let manifest = package["manifest_path"]
            .as_str()
            .context("Cargo metadata 缺少本地包清单")?;
        paths.insert(id, Path::new(manifest).canonicalize()?);
    }
    let app_manifest = app.join("Cargo.toml");
    let application = paths
        .iter()
        .find_map(|(&id, path)| (path == &app_manifest).then_some(id))
        .context("Cargo metadata 未包含当前应用")?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .context("Cargo metadata 缺少完整依赖图")?;
    let graph = nodes
        .iter()
        .filter_map(|node| Some((node["id"].as_str()?, node["dependencies"].as_array()?)))
        .collect::<HashMap<_, _>>();
    let mut visited = HashSet::new();
    let mut pending = vec![application];
    let mut manifests = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(path) = paths.get(id) {
            manifests.insert(path.clone());
        }
        let dependencies = graph.get(id).context("Cargo 依赖图缺少可达包")?;
        pending.extend(dependencies.iter().filter_map(Value::as_str));
    }
    Ok(manifests)
}

fn enclosing_workspace(manifest: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = manifest.parent().context("Cargo 清单没有父目录")?;
    let document = fs::read_to_string(manifest)?.parse::<toml_edit::DocumentMut>()?;
    if let Some(path) = document
        .get("package")
        .and_then(|package| package.get("workspace"))
        .and_then(toml_edit::Item::as_str)
    {
        return Ok(Some(root.join(path).canonicalize()?));
    }
    if document.get("workspace").is_some() {
        return Ok(Some(root.to_owned()));
    }
    for ancestor in root.ancestors().skip(1) {
        let manifest = ancestor.join("Cargo.toml");
        if manifest.is_file()
            && fs::read_to_string(manifest)?
                .parse::<toml_edit::DocumentMut>()?
                .get("workspace")
                .is_some()
        {
            return Ok(Some(ancestor.to_owned()));
        }
    }
    Ok(None)
}

fn physical_path(path: &Path) -> PathBuf {
    // Cargo 的 target 目录可以尚不存在。规范化其首个存在的父路径以对齐监听器路径。
    path.ancestors()
        .find_map(|parent| {
            let rest = path.strip_prefix(parent).ok()?;
            Some(parent.canonicalize().ok()?.join(rest))
        })
        .unwrap_or_else(|| path.to_owned())
}

fn ignore_pattern(relative: &Path) -> Option<String> {
    let mut pattern = String::new();
    for part in relative {
        pattern.push('/');
        for ch in part.to_str()?.chars() {
            if matches!(ch, '\\' | '*' | '?' | '[' | ']' | '{' | '}' | ' ') {
                pattern.push('\\');
            }
            pattern.push(ch);
        }
    }
    pattern.push('/');
    Some(pattern)
}

#[cfg(test)]
#[path = "../../tests/unit/dev_watch_files.rs"]
mod tests;
