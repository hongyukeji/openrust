//! Cargo build-script support for an application's compiled OpenRust packages.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;

/// Generate `OUT_DIR/packages.rs` for the current target and features.
pub fn generate() -> io::Result<()> {
    let app = directory("CARGO_MANIFEST_DIR")?;
    let output = directory("OUT_DIR")?;
    let target = env::var("TARGET").map_err(io::Error::other)?;
    let manifest = app.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let declarations = metadata(&manifest, &["--no-deps"], &[])?;
    let host = host_package(&declarations, &manifest)?;
    let active: Vec<_> = host["features"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.as_str())
        .filter(|name| {
            env::var_os(format!(
                "CARGO_FEATURE_{}",
                name.replace('-', "_").to_ascii_uppercase()
            ))
            .is_some()
        })
        .collect();
    let graph = metadata(
        &manifest,
        &[
            "--locked",
            "--filter-platform",
            &target,
            "--no-default-features",
        ],
        &active,
    )?;
    let native = native_views(&graph, &manifest, &output)?;
    let workspace = graph["workspace_root"]
        .as_str()
        .ok_or_else(|| invalid("Cargo metadata has no workspace root"))?;
    println!(
        "cargo:rerun-if-changed={}",
        Path::new(workspace).join("Cargo.lock").display()
    );
    fs::write(
        output.join("packages.rs"),
        format!("{}\n{native}", render(&graph, &manifest, &active)?),
    )
}

// 只遍历目标应用实际依赖的 normal 图，不扫描不相关 workspace 包或启动应用。
fn native_views(graph: &Value, manifest: &Path, output: &Path) -> io::Result<String> {
    let host = host_package(graph, manifest)?;
    let nodes = array(&graph["resolve"]["nodes"], "resolve.nodes")?;
    let packages = array(&graph["packages"], "packages")?;
    let mut pending = vec![string(&host["id"], "host ID")?];
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let package = packages
            .iter()
            .find(|p| p["id"].as_str() == Some(id))
            .ok_or_else(|| invalid("缺少模板所属包"))?;
        let root = Path::new(string(&package["manifest_path"], "manifest_path")?)
            .parent()
            .ok_or_else(|| invalid("包目录无效"))?;
        roots.push(root.join("resources/views"));
        let paths = &package["metadata"]["openrust"]["blade"]["paths"];
        if !paths.is_null() {
            for path in array(paths, "openrust.blade.paths")? {
                let path = Path::new(string(path, "模板目录")?);
                if path.is_absolute()
                    || path
                        .components()
                        .any(|p| !matches!(p, std::path::Component::Normal(_)))
                {
                    return Err(invalid("原生模板目录须为包内相对路径"));
                }
                roots.push(root.join(path));
            }
        }
        if let Some(node) = nodes.iter().find(|n| n["id"].as_str() == Some(id)) {
            for dep in array(&node["deps"], "node.deps")? {
                if array(&dep["dep_kinds"], "dep_kinds")?
                    .iter()
                    .any(|kind| kind["kind"].is_null())
                {
                    pending.push(string(&dep["pkg"], "dependency ID")?);
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    openrust_blade_compiler::build::generate(output, &roots)
}

fn directory(key: &str) -> io::Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| invalid(format!("missing Cargo {key}")))
}

fn metadata(manifest: &Path, arguments: &[&str], features: &[&str]) -> io::Result<Value> {
    let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cargo
        .args(["metadata", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(manifest)
        .args(arguments);
    if !features.is_empty() {
        cargo.arg("--features").arg(features.join(","));
    }
    let output = cargo.output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

fn host_package<'a>(graph: &'a Value, manifest: &Path) -> io::Result<&'a Value> {
    array(&graph["packages"], "packages")?
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == manifest)
        })
        .ok_or_else(|| invalid("Cargo metadata omits the current application"))
}

fn key(dependency: &Value) -> Option<&str> {
    dependency["rename"]
        .as_str()
        .or_else(|| dependency["name"].as_str())
}

// Expand the host's feature definitions to distinguish its optional dependencies
// from ones activated only by another workspace member. Reuses the v0.x rules
// for dep:name, feature forwarding, and weak forwarding.
fn active_optional<'a>(host: &'a Value, features: &[&'a str]) -> io::Result<BTreeSet<&'a str>> {
    let mut visited = BTreeSet::new();
    let mut pending = features.to_vec();
    let mut dependencies = BTreeSet::new();
    while let Some(feature) = pending.pop() {
        if !visited.insert(feature) {
            continue;
        }
        for definition in array(&host["features"][feature], "feature definitions")? {
            let definition = string(definition, "feature definition")?;
            if let Some(dependency) = definition.strip_prefix("dep:") {
                dependencies.insert(dependency);
            } else if let Some((dependency, _)) = definition.split_once('/') {
                if !dependency.ends_with('?') {
                    dependencies.insert(dependency);
                }
            } else {
                pending.push(definition);
            }
        }
    }
    Ok(dependencies)
}

fn discovery(package: &Value) -> io::Result<&Value> {
    let options = &package["metadata"]["openrust"]["discovery"];
    if !options.is_null()
        && (options["version"].as_u64() != Some(1)
            || (!options["providers"].is_null() && !options["providers"].is_boolean()))
    {
        return Err(invalid(format!("包 {} 的发现声明无效", package["name"])));
    }
    Ok(options)
}

fn exclusions(options: &Value) -> io::Result<BTreeSet<&str>> {
    let list = &options["dont-discover"];
    if list.is_null() {
        return Ok(BTreeSet::new());
    }
    array(list, "dont-discover")?
        .iter()
        .map(|value| string(value, "excluded package"))
        .collect()
}

fn render(graph: &Value, manifest: &Path, active: &[&str]) -> io::Result<String> {
    let host = host_package(graph, manifest)?;
    let optional = active_optional(host, active)?;
    let nodes = array(&graph["resolve"]["nodes"], "resolve.nodes")?;
    let node = nodes
        .iter()
        .find(|node| node["id"] == host["id"])
        .ok_or_else(|| invalid("Cargo metadata omits the application's dependency graph"))?;
    let packages: BTreeMap<_, _> = array(&graph["packages"], "packages")?
        .iter()
        .map(|package| Ok((string(&package["id"], "package ID")?, package)))
        .collect::<io::Result<_>>()?;
    let declared = array(&host["dependencies"], "dependencies")?;
    let mut candidates = Vec::new();
    let app_exclusions = exclusions(discovery(host)?)?;
    let mut excluded = app_exclusions.clone();
    for link in array(&node["deps"], "resolved dependencies")? {
        if !array(&link["dep_kinds"], "dependency kinds")?
            .iter()
            .any(|kind| kind["kind"].is_null())
        {
            continue;
        }
        let id = string(&link["pkg"], "resolved package ID")?;
        let package = packages
            .get(id)
            .ok_or_else(|| invalid("resolved package is missing"))?;
        let library = string(&link["name"], "Rust dependency name")?;
        let name = string(&package["name"], "package name")?;
        let linked_library_matches = package["targets"].as_array().is_some_and(|targets| {
            targets.iter().any(|target| {
                target["name"].as_str() == Some(library)
                    && target["kind"].as_array().is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| matches!(kind.as_str(), Some("lib" | "rlib" | "dylib")))
                    })
            })
        });
        if !declared.iter().any(|dependency| {
            dependency["kind"].is_null()
                && dependency["name"].as_str() == Some(name)
                && key(dependency).is_some_and(|key| {
                    (key.replace('-', "_") == library
                        || (dependency["rename"].is_null() && linked_library_matches))
                        && (dependency["optional"].as_bool() == Some(false)
                            || optional.contains(key))
                })
        }) {
            continue;
        }
        let path = string(&package["manifest_path"], "package manifest")?;
        println!("cargo:rerun-if-changed={path}");
        let options = discovery(package)?;
        excluded.extend(exclusions(options)?);
        if !options.is_null() {
            let version = string(&package["version"], "package version")?;
            candidates.push((name, version, library, options));
        }
    }
    candidates.sort_unstable_by_key(|&(name, version, library, _)| (name, version, library));
    let mut generated = String::from(
        "pub const PACKAGES: ::openrust::foundation::PackageManifest = ::openrust::foundation::PackageManifest::new(&[\n",
    );
    let mut aliases = BTreeMap::new();
    for (name, version, library, options) in candidates {
        if app_exclusions.contains("*") || excluded.contains(name) {
            continue;
        }
        let exports = options["aliases"].as_object();
        if !options["aliases"].is_null() && exports.is_none() {
            return Err(invalid(format!("package {name} aliases must be a table")));
        }
        let providers = options["providers"].as_bool().unwrap_or(false);
        if !providers && exports.is_none_or(serde_json::Map::is_empty) {
            continue;
        }
        let crate_name = identifier(library)?;
        let factory = if providers {
            format!("::{crate_name}::openrust_providers")
        } else {
            "::std::vec::Vec::new".into()
        };
        writeln!(
            generated,
            "::openrust::foundation::DiscoveredPackage::new({name:?}, {version:?}, {library:?}, &["
        )
        .unwrap();
        for (alias, path) in exports.into_iter().flatten() {
            let path = string(path, "alias target")?;
            let alias_ident = identifier(alias)?;
            let target = path
                .split("::")
                .map(identifier)
                .collect::<io::Result<Vec<_>>>()?
                .join("::");
            if aliases
                .insert(alias_ident, format!("::{crate_name}::{target}"))
                .is_some()
            {
                return Err(invalid(format!(
                    "duplicate discovered package alias {alias}"
                )));
            }
            writeln!(generated, "({alias:?}, {path:?}),").unwrap();
        }
        writeln!(generated, "], {factory}),").unwrap();
    }
    generated.push_str(
        "]).withNativeViews(NATIVE_VIEWS);\n#[allow(unused_imports)]\npub mod package_aliases {\n",
    );
    for (alias, target) in aliases {
        writeln!(generated, "pub use {target} as {alias};").unwrap();
    }
    generated.push_str("}\n");
    Ok(generated)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn array<'a>(value: &'a Value, label: &str) -> io::Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("{label} must be an array")))
}

fn string<'a>(value: &'a Value, label: &str) -> io::Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{label} must be a string")))
}

fn identifier(name: &str) -> io::Result<String> {
    let mut bytes = name.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || matches!(name, "_" | "crate" | "self" | "Self" | "super")
    {
        return Err(invalid(format!("invalid Rust identifier {name:?}")));
    }
    Ok(format!("r#{name}"))
}
