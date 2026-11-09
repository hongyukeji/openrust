//! 从 Cargo 图生成 Provider 包清单；发现阶段不执行包代码。

use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

mod source;
use source::Sources;

#[allow(dead_code, reason = "仅应用构建脚本消费清单")]
#[path = "../templates/bootstrap/package_discovery_build.rs"]
mod build_plan;
use build_plan::cargo_config::CargoConfig;

#[derive(Debug, clap::Args)]
pub(crate) struct DiscoverArgs {
    #[command(flatten)]
    mode: DiscoverMode,
    #[command(flatten)]
    selection: SelectionArgs,
}

#[derive(Debug, clap::Args)]
pub(crate) struct SelectionArgs {
    #[arg(long, hide = true)]
    project_dir: Option<PathBuf>,
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

#[derive(Debug, clap::Args)]
#[group(multiple = false)]
struct DiscoverMode {
    /// 仅将候选 Rust 源码写到标准输出。
    #[arg(long)]
    preview: bool,
    /// 显式重建当前清单，与不指定模式相同。
    #[arg(long)]
    install: bool,
    /// 检查当前清单是否存在且与重新生成的内容一致。
    #[arg(long)]
    check: bool,
}

impl DiscoverArgs {
    pub(crate) fn run(self) -> anyhow::Result<ExitCode> {
        let (project, target) = self.selection.project_and_target()?;
        let features = &self.selection.features;
        let no_default = self.selection.no_default_features;
        if self.mode.preview {
            let (plan, _) = inspect(&project, &target, features, no_default)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            io::stdout().lock().write_all(plan.source.as_bytes())?;
        } else {
            let (relative, contents) = prepare_plan(&project, &target, features, no_default)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            if !plan_matches(&project.join(&relative), contents.as_bytes())? {
                if self.mode.install || !self.mode.check {
                    write_plan(&project, &relative, contents.as_bytes())?;
                } else {
                    anyhow::bail!(
                        "Provider 清单缺失或已过期（{}），请以相同目标和应用特性运行 package:discover 重建",
                        relative.display()
                    );
                }
            }
            writeln!(io::stdout().lock(), "{}", relative.display())?;
        }
        Ok(ExitCode::SUCCESS)
    }
}

impl SelectionArgs {
    fn project_and_target(&self) -> anyhow::Result<(PathBuf, String)> {
        let cwd = self
            .project_dir
            .clone()
            .map_or_else(std::env::current_dir, Ok)?;
        let workspace = super::framework_workspace(&cwd)?;
        let (project, _) = super::application_directory(&workspace, &cwd)?;
        let target = self
            .target
            .as_deref()
            .filter(|target| *target != "host-tuple")
            .map_or_else(|| host_target(&project), |target| Ok(target.to_owned()))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok((project, target))
    }

    pub(crate) fn list(self) -> anyhow::Result<ExitCode> {
        let (project, target) = self.project_and_target()?;
        let (plan, features) = inspect(&project, &target, &self.features, self.no_default_features)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let target = if custom_target(&target) {
            "custom"
        } else {
            &target
        };
        let report = serde_json::json!({
            "target": target,
            "features": features,
            "global_opt_out": plan.global_opt_out,
            "packages": plan.packages,
            "aliases": plan.aliases,
        });
        let mut stdout = io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &report)?;
        writeln!(stdout)?;
        Ok(ExitCode::SUCCESS)
    }
}

fn inspect(
    project: &Path,
    target: &str,
    features: &[String],
    no_default: bool,
) -> Result<(RenderedPlan, Vec<String>)> {
    let config = CargoConfig::read(project)?;
    let graph = cargo_graph(project, target, features, no_default)?;
    let manifest = project.join("Cargo.toml");
    let host = host_package(&graph, &manifest)?;
    let active = cargo_active_features(project, target, host, features, no_default)?;
    let mut selection = active.clone();
    selection.extend_from_slice(features);
    let sources = Sources::load(workspace_root(&graph)?, &config)?;
    let plan = plan(&graph, &manifest, target, &selection, true, &sources)?;
    Ok((plan, active))
}

fn cargo_graph(
    app: &Path,
    target: &str,
    features: &[String],
    no_default_features: bool,
) -> Result<Value> {
    let manifest = app.join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .current_dir(app)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--filter-platform", target]);
    cargo_selection(&mut command, features, no_default_features);
    let output = cargo_stdout(&mut command)?;
    Ok(serde_json::from_slice(&output)?)
}

fn cargo_selection(command: &mut Command, features: &[String], no_default_features: bool) {
    if no_default_features {
        command.arg("--no-default-features");
    }
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
}

fn cargo_stdout(command: &mut Command) -> Result<Vec<u8>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "无法读取 Cargo 包发现图：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

fn cargo_active_features(
    app: &Path,
    target: &str,
    host: &Value,
    features: &[String],
    no_default_features: bool,
) -> Result<Vec<String>> {
    // metadata 合并 workspace 特性。tree 仅选择宿主包与当前构建目标。
    let package = string(&host["id"], "host package id")?;
    let mut command = Command::new("cargo");
    command.current_dir(app).args([
        "tree",
        "--locked",
        "--offline",
        "--package",
        package,
        "--target",
        target,
        "--edges",
        "normal,build",
        "--depth",
        "0",
        "--prefix",
        "none",
        "--format",
        "{f}",
        "--color",
        "never",
    ]);
    cargo_selection(&mut command, features, no_default_features);
    let output = String::from_utf8(cargo_stdout(&mut command)?)?;
    let actual = output.trim();
    if actual.contains(['\n', '\r']) {
        return Err("Cargo 返回多个应用特性集合".into());
    }
    Ok(actual
        .split(',')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

fn prepare_plan(
    app: &Path,
    target: &str,
    features: &[String],
    no_default_features: bool,
) -> Result<(PathBuf, String)> {
    if custom_target(target) {
        return Err("安装清单需要 Cargo 目标三元组".into());
    }
    let bootstrap = fs::read_to_string(app.join("bootstrap/app.rs"))?;
    let generated_helper = app.join("bootstrap/package_discovery_build.rs");
    let helper = if generated_helper.is_file() {
        generated_helper
    } else {
        app.join("tools/cli/templates/bootstrap/package_discovery_build.rs")
    };
    if !bootstrap.contains("/openrust_package_providers.rs")
        || !bootstrap.contains("PackageProvider")
        || !app.join("build.rs").is_file()
        || !helper.with_file_name("package_cargo_config.rs").is_file()
        || !fs::read_to_string(&helper).is_ok_and(|source| source.contains(build_plan::VERSION))
    {
        return Err(
            "当前应用缺少 Provider v3 清单接线，请按当前模板更新构建依赖、构建脚本和 bootstrap 接线"
                .into(),
        );
    }
    let manifest = app.join("Cargo.toml");
    let config = CargoConfig::read(app)?;
    let config_digest = config.fingerprint();
    let graph = cargo_graph(app, target, features, no_default_features)?;
    let host = host_package(&graph, &manifest)?;
    let name = string(&host["name"], "host package name")?;
    if let Some(feature) = features.iter().find(|feature| {
        feature
            .split_once('/')
            .is_some_and(|(package, _)| package != name)
    }) {
        return Err(format!(
            "安装清单无法独立识别依赖特性 `{feature}`，请用应用 Cargo 特性转发后再安装"
        )
        .into());
    }
    let inputs = input_digests(app, &graph, host)?;
    let active = cargo_active_features(app, target, host, features, no_default_features)?;
    let profile = cargo_feature_keys(host, &active)?;
    let sources = Sources::load(workspace_root(&graph)?, &config)?;
    let source = plan(&graph, &manifest, target, &active, true, &sources)?.source;
    let checked = cargo_graph(app, target, features, no_default_features)?;
    if graph != checked
        || config_digest != CargoConfig::read(app)?.fingerprint()
        || inputs != input_digests(app, &checked, host_package(&checked, &manifest)?)?
    {
        return Err("生成期间 Cargo 包发现输入变化，请重新执行安装".into());
    }

    let mut contents = format!(
        "{}\n{}{config_digest}\n",
        build_plan::VERSION,
        build_plan::CARGO_CONFIG
    );
    for (path, digest) in inputs {
        writeln!(contents, "// {digest}\t{path}")?;
    }
    writeln!(contents, "{}\n{source}", build_plan::END_INPUTS)?;
    let relative = build_plan::plan_path(target, profile);
    Ok((relative, contents))
}

fn plan_matches(path: &Path, expected: &[u8]) -> io::Result<bool> {
    match fs::read(path) {
        Ok(current) => Ok(current == expected),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn write_plan(app: &Path, relative: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = app.join(build_plan::DIRECTORY);
    fs::create_dir_all(&parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.persist(app.join(relative))?;
    Ok(())
}

fn workspace_root(graph: &Value) -> Result<&Path> {
    Ok(Path::new(string(
        &graph["workspace_root"],
        "workspace root",
    )?))
}

fn input_digests(app: &Path, graph: &Value, host: &Value) -> Result<BTreeMap<String, String>> {
    let workspace = workspace_root(graph)?;
    let node = host_node(graph, host)?;
    let direct: Vec<_> = array(&node["deps"], "resolve node deps")?
        .iter()
        .filter(|edge| {
            edge["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()))
        })
        .map(|edge| &edge["pkg"])
        .collect();
    let mut paths = BTreeSet::from([
        app.join("Cargo.toml"),
        workspace.join("Cargo.toml"),
        workspace.join("Cargo.lock"),
    ]);
    for package in array(&graph["packages"], "packages")? {
        if package["source"].is_null() && direct.contains(&&package["id"]) {
            paths.insert(PathBuf::from(string(
                &package["manifest_path"],
                "package manifest",
            )?));
        }
    }
    let mut inputs = BTreeMap::new();
    for path in paths {
        inputs.insert(
            relative_input(app, &path)?,
            build_plan::digest(&fs::read(&path)?),
        );
    }
    Ok(inputs)
}

fn relative_input(app: &Path, path: &Path) -> Result<String> {
    let common = app
        .ancestors()
        .find(|parent| path.starts_with(parent))
        .ok_or("包发现清单输入与应用必须位于同一卷")?;
    let mut relative = PathBuf::new();
    for _ in app.strip_prefix(common)?.components() {
        relative.push("..");
    }
    relative.push(path.strip_prefix(common)?);
    relative
        .to_str()
        .filter(|value| !value.contains(['\n', '\r', '\t']))
        .map(|value| value.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| "包发现清单需要无换行及制表符的 UTF-8 输入路径".into())
}

fn cargo_feature_keys(host: &Value, active: &[String]) -> Result<Vec<String>> {
    let features = host["features"]
        .as_object()
        .ok_or("Cargo 图缺少当前应用特性")?;
    let mut by_key = BTreeMap::new();
    for name in features.keys() {
        let key = build_plan::feature_key(name);
        if let Some(other) = by_key.insert(key, name.as_str()) {
            return Err(
                format!("Cargo 特性 `{other}` 与 `{name}` 在构建环境中同名，无法安装清单").into(),
            );
        }
    }
    active
        .iter()
        .map(|name| {
            let key = build_plan::feature_key(name);
            if by_key.get(&key) != Some(&name.as_str()) {
                return Err(format!("Cargo 返回未声明的应用特性 `{name}`").into());
            }
            Ok(key)
        })
        .collect()
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

fn custom_target(target: &str) -> bool {
    target.contains(['/', '\\'])
        || Path::new(target)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn plan(
    graph: &Value,
    manifest: &Path,
    target: &str,
    features: &[String],
    no_default_features: bool,
    sources: &Sources,
) -> Result<RenderedPlan> {
    let packages = array(&graph["packages"], "packages")?;
    let host = host_package(graph, manifest)?;
    if exclusions(&host["metadata"]["openrust"]["discovery"])?.contains("*") {
        return render(host, &[]);
    }
    let node = host_node(graph, host)?;
    let optional = active_optional_dependencies(host, features, no_default_features)?;
    let declarations = declarations(host, node, packages, &optional, target, sources)?;
    render(host, &declarations)
}

fn host_package<'a>(graph: &'a Value, manifest: &Path) -> Result<&'a Value> {
    array(&graph["packages"], "packages")?
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == manifest)
        })
        .ok_or_else(|| "Cargo 图不包含当前应用清单".into())
}

fn host_node<'a>(graph: &'a Value, host: &Value) -> Result<&'a Value> {
    array(&graph["resolve"]["nodes"], "resolve.nodes")?
        .iter()
        .find(|node| node["id"] == host["id"])
        .ok_or_else(|| "Cargo 图不包含当前应用节点".into())
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
    version: &'a str,
    library: &'a str,
    source: &'static str,
    options: &'a Value,
    activation: Activation,
}

enum Activation {
    Active(String),
    InactiveFeature,
    TargetMismatch,
}

impl Activation {
    fn condition(&self) -> Option<&str> {
        match self {
            Self::Active(condition) => Some(condition),
            Self::InactiveFeature | Self::TargetMismatch => None,
        }
    }
}

fn source_kind(package: &Value) -> &'static str {
    match package["source"]
        .as_str()
        .and_then(|source| source.split_once('+'))
    {
        Some(("git", _)) => "git",
        Some(("registry" | "sparse", _)) => "registry",
        _ if package["source"].is_null() => "path",
        _ => "unknown",
    }
}

fn matches_declaration(
    dependency: &Value,
    package: &Value,
    library: &str,
    kinds: &[Value],
    version: &semver::Version,
) -> Result<bool> {
    let alias = dependency["rename"].as_str();
    Ok(dependency["name"] == package["name"]
        && dependency["kind"].is_null()
        && alias.map_or_else(
            || unrenamed_library(package, library),
            |alias| alias.replace('-', "_") == library,
        )
        && kinds
            .iter()
            .any(|kind| kind["kind"].is_null() && kind["target"] == dependency["target"])
        && matches_dependency_version(dependency, version)?)
}

fn declarations<'a>(
    host: &Value,
    node: &'a Value,
    packages: &'a [Value],
    optional: &BTreeSet<&str>,
    target: &str,
    sources: &Sources,
) -> Result<Vec<Declaration<'a>>> {
    let mut declarations = Vec::new();
    let mut assignments = BTreeMap::new();
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
        let discoverable = !options.is_null();
        let version_text = string(&package["version"], "package version")?;
        let version = semver::Version::parse(version_text)?;
        let mut conditions = BTreeSet::new();
        let mut plausible = false;
        let mut matched = false;
        let mut enabled = false;
        for (index, dependency) in dependencies.iter().enumerate() {
            if !matches_declaration(dependency, package, library, kinds, &version)? {
                continue;
            }
            let alias = dependency["rename"].as_str();
            plausible = true;
            if !sources.matches(dependency, package, &version)? {
                continue;
            }
            matched = true;
            if dependency["optional"].as_bool() == Some(true)
                && !optional.contains(alias.unwrap_or(name))
            {
                continue;
            }
            // 候选包可能没有发现声明，但仍会让该 extern 关联变得不唯一。
            if let Some((first, first_discoverable)) =
                assignments.insert(index, (&edge["pkg"], discoverable))
                && first != &edge["pkg"]
                && (discoverable || first_discoverable)
            {
                return Err(
                    format!("依赖 `{}` 的 Provider 包来源有歧义", alias.unwrap_or(name)).into(),
                );
            }
            if !discoverable {
                continue;
            }
            enabled = true;
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
        if discoverable && plausible && !matched {
            return Err(format!(
                "无法确认 Provider 包 `{name}` 的 Cargo 来源，请检查 workspace 的 [patch] 声明"
            )
            .into());
        }
        if !discoverable || !matched {
            continue;
        }
        let activation = if !conditions.is_empty() {
            check_declaration(options, name)?;
            Activation::Active(disjunction(conditions.into_iter()))
        } else if enabled {
            Activation::TargetMismatch
        } else {
            Activation::InactiveFeature
        };
        declarations.push(Declaration {
            package: name,
            version: version_text,
            library,
            source: source_kind(package),
            options,
            activation,
        });
    }
    declarations.sort_by_key(|declaration| (declaration.package, declaration.library));
    Ok(declarations)
}

fn matches_dependency_version(dependency: &Value, version: &semver::Version) -> Result<bool> {
    let requirement = string(&dependency["req"], "dependency version")?;
    Ok(requirement == "*" || semver::VersionReq::parse(requirement)?.matches(version))
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

struct RenderedPlan {
    source: String,
    global_opt_out: bool,
    packages: Vec<Value>,
    aliases: Vec<Value>,
}

struct ExclusionRule<'a> {
    package: &'a str,
    library: &'a str,
    condition: &'a str,
}

fn exclusion_rules<'a>(
    declarations: &'a [Declaration<'a>],
) -> Result<BTreeMap<&'a str, Vec<ExclusionRule<'a>>>> {
    let mut rules = BTreeMap::<_, Vec<_>>::new();
    for declaration in declarations {
        if let Some(condition) = declaration.activation.condition() {
            for name in exclusions(declaration.options)? {
                rules.entry(name).or_default().push(ExclusionRule {
                    package: declaration.package,
                    library: declaration.library,
                    condition,
                });
            }
        }
    }
    Ok(rules)
}

fn package_selection(
    declaration: &Declaration<'_>,
    excluded: &BTreeSet<&str>,
    rules: &[ExclusionRule<'_>],
) -> (&'static str, Option<String>) {
    let active = match &declaration.activation {
        Activation::Active(active) => active,
        Activation::InactiveFeature => return ("inactive_feature", None),
        Activation::TargetMismatch => return ("target_mismatch", None),
    };
    if excluded.contains("*") || excluded.contains(declaration.package) {
        return ("excluded_by_application", None);
    }
    if rules.is_empty() {
        return ("selected", Some(active.clone()));
    }
    let excluded_when = disjunction(rules.iter().map(|rule| rule.condition));
    let reason = if rules
        .iter()
        .any(|rule| rule.condition == "all()" || rule.condition == active)
    {
        "excluded_by_package"
    } else {
        "conditional_exclusion"
    };
    (reason, Some(format!("all({active}, not({excluded_when}))")))
}

fn render(host: &Value, declarations: &[Declaration<'_>]) -> Result<RenderedPlan> {
    let host_options = &host["metadata"]["openrust"]["discovery"];
    if !host_options.is_null() {
        check_declaration(host_options, string(&host["name"], "host package name")?)?;
    }
    let excluded = exclusions(host_options)?;
    let package_exclusions = exclusion_rules(declarations)?;
    let mut package_report = Vec::new();
    let mut providers = String::new();
    let mut aliases: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for declaration in declarations {
        let rules = package_exclusions
            .get(declaration.package)
            .map_or(&[][..], Vec::as_slice);
        let (decision, active) = package_selection(declaration, &excluded, rules);
        let provider = declaration.options["providers"].as_bool() == Some(true);
        package_report.push(serde_json::json!({
            "package": declaration.package,
            "version": declaration.version,
            "library": declaration.library,
            "source": declaration.source,
            "decision": decision,
            "condition": active,
            "provider": provider,
            "excluded_by": rules.iter().map(|rule| serde_json::json!({
                "package": rule.package,
                "library": rule.library,
                "condition": rule.condition,
            })).collect::<Vec<_>>(),
        }));
        let Some(active) = active else {
            continue;
        };
        let library = rust_identifier(declaration.library)?;
        if provider {
            let origin = format!(
                "{}@{} ({})",
                declaration.package, declaration.version, declaration.library
            );
            writeln!(
                providers,
                "        #[cfg({active})]\n        ({origin:?}, ::{library}::openrust_providers),"
            )?;
        }
        collect_aliases(declaration.options, &library, &active, &mut aliases)?;
    }
    let mut source = format!(
        "pub(crate) fn providers() -> Vec<::openrust::foundation::PackageProvider> {{\n    type Providers = Vec<Box<dyn ::openrust::support::ServiceProvider>>;\n    type Factory = (&'static str, fn() -> Providers);\n    let factories: &[Factory] = &[\n{providers}    ];\n    factories\n        .iter()\n        .flat_map(|&(origin, factory)| {{\n            factory().into_iter().map(move |provider| {{\n                ::openrust::foundation::PackageProvider::new(origin, provider)\n            }})\n        }})\n        .collect()\n}}\n"
    );
    source.push_str("#[allow(unused_imports, reason = \"应用按需引用发现的 facade\")]\npub(crate) mod facades {\n");
    let mut alias_report = Vec::new();
    for (alias, candidates) in aliases {
        let mut report = Vec::new();
        for (index, (active, path)) in candidates.iter().enumerate() {
            let later = disjunction(
                candidates[index + 1..]
                    .iter()
                    .map(|(active, _)| active.as_str()),
            );
            let condition = format!("all({active}, not({later}))");
            writeln!(
                source,
                "    #[cfg({condition})]\n    pub use {path} as {alias};"
            )?;
            report.push(serde_json::json!({"path": path, "condition": condition}));
        }
        alias_report.push(serde_json::json!({
            "name": alias.trim_start_matches("r#"),
            "candidates": report,
        }));
    }
    source.push_str("}\n");
    Ok(RenderedPlan {
        source,
        global_opt_out: excluded.contains("*"),
        packages: package_report,
        aliases: alias_report,
    })
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
