//! Provider 包发现预览。只从 Cargo 图生成普通 Rust 引用，不执行包代码。

use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::{self, Write as _},
    path::Path,
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, clap::Args)]
pub(crate) struct DiscoverArgs {
    /// 仅将候选 Rust 源码写到标准输出。
    #[arg(long, required = true)]
    preview: bool,
    /// 指定 Cargo 目标，默认使用当前 Rust 工具链的宿主目标。
    #[arg(long)]
    target: Option<String>,
    /// 逗号分隔的应用 Cargo 特性，可重复指定。
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,
    /// 排除应用默认 Cargo 特性。
    #[arg(long)]
    no_default_features: bool,
}

impl DiscoverArgs {
    pub(crate) fn run(self) -> anyhow::Result<ExitCode> {
        anyhow::ensure!(self.preview, "请使用 --preview 输出 Provider 包发现预览");
        let cwd = std::env::current_dir()?;
        let workspace = super::framework_workspace(&cwd)?;
        let (project, _) = super::application_directory(&workspace, &cwd)?;
        let source = preview(
            &project,
            self.target.as_deref(),
            &self.features,
            self.no_default_features,
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        io::stdout().lock().write_all(source.as_bytes())?;
        Ok(ExitCode::SUCCESS)
    }
}

fn preview(
    app: &Path,
    target: Option<&str>,
    features: &[String],
    no_default_features: bool,
) -> Result<String> {
    let manifest = app.join("Cargo.toml");
    let target = target.map_or_else(|| host_target(app), |target| Ok(target.to_owned()))?;
    let mut command = Command::new("cargo");
    command
        .current_dir(app)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--filter-platform", &target]);
    if no_default_features {
        command.arg("--no-default-features");
    }
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "无法读取 Cargo 包发现图：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let graph: Value = serde_json::from_slice(&output.stdout)?;
    plan(&graph, &manifest, &target, features, no_default_features)
}

fn host_target(app: &Path) -> Result<String> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc).current_dir(app).arg("-vV").output()?;
    if !output.status.success() {
        return Err("无法确定 Rust 宿主目标，请显式指定 --target".into());
    }
    let info = String::from_utf8(output.stdout)?;
    info.lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "无法确定 Rust 宿主目标，请显式指定 --target".into())
}

fn plan(
    graph: &Value,
    manifest: &Path,
    target: &str,
    features: &[String],
    no_default_features: bool,
) -> Result<String> {
    let packages = array(&graph["packages"], "packages")?;
    let host = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == manifest)
        })
        .ok_or("Cargo 图不包含当前应用清单")?;
    if exclusions(&host["metadata"]["openrust"]["discovery"])?.contains("*") {
        return render(host, &[]);
    }
    let nodes = array(&graph["resolve"]["nodes"], "resolve.nodes")?;
    let node = nodes
        .iter()
        .find(|node| node["id"] == host["id"])
        .ok_or("Cargo 图不包含当前应用节点")?;
    let optional = active_optional_dependencies(host, features, no_default_features)?;
    let declarations = declarations(host, node, packages, &optional, target)?;
    render(host, &declarations)
}

fn active_optional_dependencies<'a>(
    host: &'a Value,
    selected: &'a [String],
    no_default_features: bool,
) -> Result<BTreeSet<&'a str>> {
    let host_name = string(&host["name"], "host package name")?;
    let features = host["features"]
        .as_object()
        .ok_or("Cargo 图缺少当前应用特性")?;
    let declared = array(&host["dependencies"], "dependencies")?;
    let mut pending = Vec::new();
    let mut dependencies = BTreeSet::new();
    for feature in selected {
        match feature.split_once('/') {
            None => pending.push(feature.as_str()),
            Some((package, name)) if package == host_name => pending.push(name),
            Some((package, _))
                if declared.iter().any(|dependency| {
                    dependency_key(dependency) == Some(package.trim_end_matches('?'))
                }) =>
            {
                if !package.ends_with('?') {
                    dependencies.insert(package);
                }
            }
            Some(_) => return Err(format!("特性 `{feature}` 不属于当前应用或其直接依赖").into()),
        }
    }
    if !no_default_features && features.contains_key("default") {
        pending.push("default");
    }
    let mut visited = BTreeSet::new();
    while let Some(feature) = pending.pop() {
        if !visited.insert(feature) {
            continue;
        }
        let definitions = features
            .get(feature)
            .ok_or_else(|| format!("未声明的应用 Cargo 特性 `{feature}`"))?;
        for definition in array(definitions, "feature values")? {
            let definition = string(definition, "feature value")?;
            if let Some(name) = definition.strip_prefix("dep:") {
                dependencies.insert(name);
            } else if let Some((name, _)) = definition.split_once('/') {
                if !name.ends_with('?') {
                    dependencies.insert(name);
                }
            } else {
                pending.push(definition);
            }
        }
    }
    Ok(dependencies)
}

fn dependency_key(dependency: &Value) -> Option<&str> {
    dependency["rename"]
        .as_str()
        .or_else(|| dependency["name"].as_str())
}

fn unrenamed_library(package: &Value, library: &str) -> bool {
    package["targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            target["name"].as_str() == Some(library)
                && target["kind"].as_array().is_some_and(|kinds| {
                    kinds.iter().any(|kind| {
                        matches!(kind.as_str(), Some("lib" | "rlib" | "dylib" | "proc-macro"))
                    })
                })
        })
    })
}

struct Declaration<'a> {
    package: &'a str,
    library: &'a str,
    options: &'a Value,
    active: String,
}

fn declarations<'a>(
    host: &Value,
    node: &'a Value,
    packages: &'a [Value],
    optional: &BTreeSet<&str>,
    target: &str,
) -> Result<Vec<Declaration<'a>>> {
    let mut declarations = Vec::new();
    let dependencies = array(&host["dependencies"], "dependencies")?;
    for edge in array(&node["deps"], "resolve node deps")? {
        let kinds = array(&edge["dep_kinds"], "dependency kinds")?;
        if !kinds.iter().any(|kind| kind["kind"].is_null()) {
            continue;
        }
        let package = packages
            .iter()
            .find(|package| package["id"] == edge["pkg"])
            .ok_or("直接依赖缺少解析到的包")?;
        let name = string(&package["name"], "package name")?;
        let library = string(&edge["name"], "library name")?;
        let options = &package["metadata"]["openrust"]["discovery"];
        if options.is_null() {
            continue;
        }
        let mut conditions = BTreeSet::new();
        for dependency in dependencies {
            let alias = dependency["rename"].as_str();
            if dependency["name"].as_str() != Some(name)
                || !dependency["kind"].is_null()
                || !alias.map_or_else(
                    || unrenamed_library(package, library),
                    |alias| alias.replace('-', "_") == library,
                )
                || !kinds
                    .iter()
                    .any(|kind| kind["kind"].is_null() && kind["target"] == dependency["target"])
                || dependency["optional"].as_bool() == Some(true)
                    && !optional.contains(alias.unwrap_or(name))
            {
                continue;
            }
            match dependency["target"].as_str() {
                None => {
                    conditions.insert("all()");
                }
                Some(platform) if platform == target => {
                    conditions.insert("all()");
                }
                Some(platform) => {
                    if let Some(cfg) = target_condition(platform)? {
                        conditions.insert(cfg);
                    }
                }
            }
        }
        if conditions.is_empty() {
            continue;
        }
        check_declaration(options, name)?;
        let active = disjunction(conditions.into_iter());
        declarations.push(Declaration {
            package: name,
            library,
            options,
            active,
        });
    }
    declarations.sort_by_key(|declaration| (declaration.package, declaration.library));
    Ok(declarations)
}

fn target_condition(platform: &str) -> Result<Option<&str>> {
    let Some(cfg) = platform
        .strip_prefix("cfg(")
        .and_then(|text| text.strip_suffix(')'))
    else {
        return Ok(None);
    };
    // Cargo 不按 rustc 的测试、宏、调试或应用 feature 来选择目标依赖。
    let has_unsupported_key = cfg.split('"').step_by(2).any(|expression| {
        expression
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .any(|key| matches!(key, "feature" | "test" | "proc_macro" | "debug_assertions"))
    });
    if has_unsupported_key {
        return Err(format!("Cargo 目标依赖条件 `{platform}` 无法用于 Provider 包发现").into());
    }
    Ok(Some(cfg))
}

fn disjunction<'a>(conditions: impl Iterator<Item = &'a str>) -> String {
    let mut conditions = conditions.peekable();
    let Some(first) = conditions.next() else {
        return "any()".into();
    };
    if conditions.peek().is_none() {
        return first.into();
    }
    format!(
        "any({first}, {})",
        conditions.collect::<Vec<_>>().join(", ")
    )
}

fn render(host: &Value, declarations: &[Declaration<'_>]) -> Result<String> {
    let host_options = &host["metadata"]["openrust"]["discovery"];
    if !host_options.is_null() {
        check_declaration(host_options, string(&host["name"], "host package name")?)?;
    }
    let excluded = exclusions(host_options)?;
    let mut package_exclusions: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for declaration in declarations {
        for name in exclusions(declaration.options)? {
            package_exclusions
                .entry(name)
                .or_default()
                .push(&declaration.active);
        }
    }
    let mut providers = String::new();
    let mut aliases: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for declaration in declarations {
        if excluded.contains("*") || excluded.contains(declaration.package) {
            continue;
        }
        let library = rust_identifier(declaration.library)?;
        let active = package_exclusions.get(declaration.package).map_or_else(
            || declaration.active.clone(),
            |conditions| {
                format!(
                    "all({}, not({}))",
                    declaration.active,
                    disjunction(conditions.iter().copied())
                )
            },
        );
        if declaration.options["providers"].as_bool() == Some(true) {
            writeln!(
                providers,
                "        #[cfg({active})]\n        ::{library}::openrust_providers,"
            )?;
        }
        collect_aliases(declaration.options, &library, &active, &mut aliases)?;
    }
    let mut source = format!(
        "pub(crate) fn providers() -> Vec<Box<dyn ::openrust::support::ServiceProvider>> {{\n    type Providers = Vec<Box<dyn ::openrust::support::ServiceProvider>>;\n    let factories: &[fn() -> Providers] = &[\n{providers}    ];\n    factories.iter().flat_map(|factory| factory()).collect()\n}}\n"
    );
    source.push_str("#[allow(unused_imports, reason = \"应用按需引用发现的 facade\")]\npub(crate) mod facades {\n");
    for (alias, candidates) in aliases {
        for (index, (active, path)) in candidates.iter().enumerate() {
            let later = disjunction(
                candidates[index + 1..]
                    .iter()
                    .map(|(active, _)| active.as_str()),
            );
            writeln!(
                source,
                "    #[cfg(all({active}, not({later})))]\n    pub use {path} as {alias};"
            )?;
        }
    }
    source.push_str("}\n");
    Ok(source)
}

fn collect_aliases(
    options: &Value,
    library: &str,
    active: &str,
    aliases: &mut BTreeMap<String, Vec<(String, String)>>,
) -> Result<()> {
    let entries = &options["aliases"];
    if !entries.is_null() {
        for (alias, path) in entries
            .as_object()
            .ok_or("包 aliases 必须是名称到路径的表")?
        {
            let alias = rust_identifier(alias)?;
            let path = string(path, "alias path")?
                .split("::")
                .map(rust_identifier)
                .collect::<Result<Vec<_>>>()?
                .join("::");
            aliases
                .entry(alias)
                .or_default()
                .push((active.into(), format!("::{library}::{path}")));
        }
    }
    Ok(())
}

fn check_declaration(options: &Value, package: &str) -> Result<()> {
    if options["version"].as_u64() != Some(1) {
        return Err(format!("包 `{package}` 的发现声明需要 version = 1").into());
    }
    if !options["providers"].is_null() && !options["providers"].is_boolean() {
        return Err(format!("包 `{package}` 的 providers 必须是布尔值").into());
    }
    Ok(())
}

fn exclusions(options: &Value) -> Result<BTreeSet<&str>> {
    let names = &options["dont-discover"];
    if names.is_null() {
        return Ok(BTreeSet::new());
    }
    array(names, "dont-discover")?
        .iter()
        .map(|name| string(name, "excluded package"))
        .collect()
}

fn array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label} 必须是数组").into())
}

fn string<'a>(value: &'a Value, label: &str) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| format!("{label} 必须是字符串").into())
}

fn rust_identifier(name: &str) -> Result<String> {
    let mut bytes = name.bytes();
    let first = bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_');
    if !first
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || matches!(name, "_" | "crate" | "self" | "Self" | "super")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无效的 Rust 标识符 `{name}`"),
        )
        .into());
    }
    Ok(format!("r#{name}"))
}

#[cfg(test)]
#[path = "../tests/unit/package_discovery.rs"]
mod tests;
