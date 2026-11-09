//! 用 Cargo 元数据与有效配置的显式 patch 证明依赖来源。

use super::{Result, build_plan::cargo_config::CargoConfig};
use semver::{Version, VersionReq};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, TableLike};
use url::{Position, Url};

const CRATES_IO_INDEX: &str = "https://github.com/rust-lang/crates.io-index";
const CRATES_IO_SPARSE: &str = "sparse+https://index.crates.io/";

#[derive(Default)]
pub(super) struct Sources {
    registries: BTreeMap<String, String>,
    patches: Vec<Patch>,
}

impl Sources {
    pub(super) fn load(workspace: &Path, config: &CargoConfig) -> Result<Self> {
        let manifest = read_toml(&workspace.join("Cargo.toml"))?.ok_or("缺少 Cargo 工作区清单")?;
        let mut manifest_patches = PatchTable::new();
        read_patches(&mut manifest_patches, &manifest, workspace);

        let mut sources = Self::default();
        let mut config_patches = PatchTable::new();
        for layer in &config.layers {
            let directory = layer
                .file
                .parent()
                .and_then(Path::parent)
                .ok_or("Cargo 配置文件缺少相对路径基准")?;
            sources.read_registries(&layer.document);
            read_patches(&mut config_patches, &layer.document, directory);
        }

        let mut patches = sources.resolve_origins(manifest_patches);
        for (origin, entries) in sources.resolve_origins(config_patches) {
            patches.entry(origin).or_default().extend(entries);
        }
        for (origin, entries) in patches {
            let origin = canonical_origin(origin);
            for (alias, fields) in entries {
                if let Some(patch) = fields.into_patch(origin.clone(), alias)? {
                    sources.patches.push(patch);
                }
            }
        }
        Ok(sources)
    }

    pub(super) fn matches(
        &self,
        dependency: &Value,
        package: &Value,
        version: &Version,
    ) -> Result<bool> {
        let name = dependency["name"]
            .as_str()
            .ok_or("Cargo 元数据缺少依赖包名")?;
        if package["name"].as_str() != Some(name) {
            return Ok(false);
        }
        let Some(declared) = self.dependency_source(dependency) else {
            return Ok(false);
        };
        let actual = if package["source"].is_null() {
            package["manifest_path"]
                .as_str()
                .and_then(|manifest| Path::new(manifest).parent())
                .map(|directory| Source::Path(directory.to_path_buf()))
        } else {
            package["source"].as_str().and_then(Source::from_metadata)
        };
        let Some(actual) = actual else {
            return Ok(false);
        };
        if declared.same_as(&actual) {
            return Ok(true);
        }
        let Some(origin) = declared.origin() else {
            return Ok(false);
        };
        Ok(self.patches.iter().any(|patch| {
            &patch.origin == origin
                && patch.package == name
                && patch.replacement.same_as(&actual)
                && patch
                    .version
                    .as_ref()
                    .is_none_or(|req| req.matches(version))
        }))
    }

    fn dependency_source(&self, dependency: &Value) -> Option<Source> {
        if let Some(path) = dependency["path"].as_str() {
            return Some(Source::Path(path.into()));
        }
        if let Some(source) = dependency["source"].as_str() {
            return Source::from_metadata(source);
        }
        let registry = dependency["registry"].as_str()?;
        self.registry_url(registry).map(Source::registry)
    }

    fn read_registries(&mut self, config: &DocumentMut) {
        let Some(registries) = config.get("registries").and_then(Item::as_table_like) else {
            return;
        };
        for (name, item) in registries.iter() {
            if let Some(index) = item
                .as_table_like()
                .and_then(|table| string(table, "index"))
            {
                self.registries.insert(name.to_owned(), index.to_owned());
            }
        }
    }

    fn registry_url(&self, name: &str) -> Option<Url> {
        if name == "crates-io" {
            return Url::parse(CRATES_IO_INDEX).ok();
        }
        Url::parse(self.registries.get(name).map_or(name, String::as_str)).ok()
    }

    fn resolve_origins(&self, table: PatchTable) -> BTreeMap<Url, BTreeMap<String, PatchFields>> {
        table
            .into_iter()
            .filter_map(|(origin, entries)| Some((self.registry_url(&origin)?, entries)))
            .collect()
    }
}

type PatchTable = BTreeMap<String, BTreeMap<String, PatchFields>>;

#[derive(Default)]
struct PatchFields {
    path: Option<PathBuf>,
    git: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    rev: Option<String>,
    package: Option<String>,
    version: Option<String>,
}

impl PatchFields {
    fn read(&mut self, item: &Item, directory: &Path) {
        if let Some(version) = item.as_str() {
            *self = Self {
                version: Some(version.to_owned()),
                ..Self::default()
            };
        } else if let Some(table) = item.as_table_like() {
            if let Some(path) = string(table, "path") {
                self.path = Some(directory.join(path));
            }
            for (key, field) in [
                ("git", &mut self.git),
                ("branch", &mut self.branch),
                ("tag", &mut self.tag),
                ("rev", &mut self.rev),
                ("package", &mut self.package),
                ("version", &mut self.version),
            ] {
                if let Some(value) = string(table, key) {
                    *field = Some(value.to_owned());
                }
            }
        }
    }

    fn into_patch(self, origin: Url, alias: String) -> Result<Option<Patch>> {
        let replacement = match (self.path, self.git) {
            (Some(path), None) => Source::Path(path),
            (None, Some(git)) => {
                let references = [
                    ("branch", self.branch),
                    ("tag", self.tag),
                    ("rev", self.rev),
                ];
                let mut references = references
                    .into_iter()
                    .filter_map(|(kind, value)| Some((kind, value?)));
                let reference = references.next();
                let Ok(url) = Url::parse(&git) else {
                    return Ok(None);
                };
                if references.next().is_some() || url.query().is_some() || url.fragment().is_some()
                {
                    return Ok(None);
                }
                Source::Git(canonical_url(url), reference)
            }
            _ => return Ok(None),
        };
        let version = self
            .version
            .as_deref()
            .map(VersionReq::parse)
            .transpose()
            .map_err(|_| "Cargo patch 的版本范围无效")?;
        Ok(Some(Patch {
            origin,
            package: self.package.unwrap_or(alias),
            replacement,
            version,
        }))
    }
}

struct Patch {
    origin: Url,
    package: String,
    replacement: Source,
    version: Option<VersionReq>,
}

enum Source {
    Path(PathBuf),
    Git(Url, Option<(&'static str, String)>),
    Registry { url: Url, sparse: bool },
}

impl Source {
    fn from_metadata(source: &str) -> Option<Self> {
        if let Some(git) = source.strip_prefix("git+") {
            let mut url = Url::parse(git).ok()?;
            if url.fragment().is_some_and(|fragment| {
                !matches!(fragment.len(), 40 | 64)
                    || !fragment.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return None;
            }
            let reference = git_reference(&url).ok()?;
            url.set_query(None);
            url.set_fragment(None);
            return Some(Self::Git(canonical_url(url), reference));
        }
        let registry = source
            .strip_prefix("registry+")
            .or_else(|| source.starts_with("sparse+").then_some(source))?;
        Some(Self::registry(Url::parse(registry).ok()?))
    }

    fn registry(url: Url) -> Self {
        // Cargo 比较规范化 URL 时仍区分普通与 sparse registry；crates.io 是同一逻辑源。
        let sparse = url.scheme().starts_with("sparse+") && url.as_str() != CRATES_IO_SPARSE;
        Self::Registry {
            url: canonical_origin(url),
            sparse,
        }
    }

    fn same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Path(left), Self::Path(right)) => {
                left == right
                    || fs::canonicalize(left)
                        .is_ok_and(|left| fs::canonicalize(right).is_ok_and(|right| left == right))
            }
            (Self::Git(left, left_ref), Self::Git(right, right_ref)) => {
                left == right && left_ref == right_ref
            }
            (
                Self::Registry {
                    url: left,
                    sparse: left_sparse,
                },
                Self::Registry {
                    url: right,
                    sparse: right_sparse,
                },
            ) => left_sparse == right_sparse && left == right,
            _ => false,
        }
    }

    fn origin(&self) -> Option<&Url> {
        match self {
            Self::Git(url, _) | Self::Registry { url, .. } => Some(url),
            Self::Path(_) => None,
        }
    }
}

fn git_reference(url: &Url) -> std::result::Result<Option<(&'static str, String)>, ()> {
    let mut pairs = url.query_pairs();
    let Some((key, value)) = pairs.next() else {
        return Ok(None);
    };
    if pairs.next().is_some() {
        return Err(());
    }
    let kind = match key.as_ref() {
        "branch" | "ref" => "branch",
        "tag" => "tag",
        "rev" => "rev",
        _ => return Err(()),
    };
    Ok(Some((kind, value.into_owned())))
}

fn canonical_origin(url: Url) -> Url {
    if url.as_str() == CRATES_IO_SPARSE
        && let Ok(index) = Url::parse(CRATES_IO_INDEX)
    {
        return index;
    }
    canonical_url(url)
}

fn canonical_url(mut url: Url) -> Url {
    if url.cannot_be_a_base() {
        return url;
    }
    if url.path().ends_with('/')
        && let Ok(mut segments) = url.path_segments_mut()
    {
        segments.pop_if_empty();
    }
    if url.host_str() == Some("github.com") {
        if let Ok(https) = Url::parse(&format!("https{}", &url[Position::AfterScheme..])) {
            url = https;
        }
        url.set_path(&url.path().to_ascii_lowercase());
    }
    if let Some(last) = url
        .path()
        .rsplit('/')
        .next()
        .and_then(|last| last.strip_suffix(".git"))
    {
        let last = last.to_owned();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop().push(&last);
        }
    }
    url
}

fn read_toml(path: &Path) -> Result<Option<DocumentMut>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取 Cargo 项目清单或配置".into()),
    };
    let document = contents
        .parse()
        .map_err(|_| "无法解析 Cargo 项目清单或配置")?;
    Ok(Some(document))
}

fn read_patches(patches: &mut PatchTable, document: &DocumentMut, directory: &Path) {
    let Some(origins) = document.get("patch").and_then(Item::as_table_like) else {
        return;
    };
    for (origin, item) in origins.iter() {
        if let Some(entries) = item.as_table_like() {
            let patches = patches.entry(origin.to_owned()).or_default();
            for (alias, item) in entries.iter() {
                patches
                    .entry(alias.to_owned())
                    .or_default()
                    .read(item, directory);
            }
        }
    }
}

fn string<'a>(table: &'a dyn TableLike, key: &str) -> Option<&'a str> {
    table.get(key).and_then(Item::as_str)
}
