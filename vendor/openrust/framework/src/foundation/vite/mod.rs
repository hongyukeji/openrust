//! 前端资源清单、开发服务器与资源标签生成。
mod facade;
mod fonts;
mod prefetch;
mod provider;
mod render;
pub use facade::ViteFacade;
pub use provider::ViteServiceProvider;

use super::Application;
use crate::{
    container::Container,
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        routing::UrlGenerator as _,
    },
    routing::UrlGenerator,
    support::{HtmlString, Str},
};
use indexmap::IndexMap;
use serde_json::Value;
use std::{
    collections::HashMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, RwLock, Weak},
};

pub type ViteResult<T> = Result<T, ViteError>;
pub type Attributes = IndexMap<String, Value>;
pub type Manifest = IndexMap<String, Value>;

#[derive(Debug)]
pub enum ViteError {
    Unavailable(BindingResolutionError),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    ManifestNotFound(PathBuf),
    InvalidManifest {
        path: PathBuf,
        source: serde_json::Error,
    },
    MissingChunk(String),
    InvalidChunk(String),
    InvalidAttribute(String),
    InvalidFonts(String),
    InvalidPrefetchStrategy(String),
    DisabledPrefetchAttributes,
    Url(crate::routing::UrlError),
    Callback(Box<dyn Error + Send + Sync>),
}
impl fmt::Display for ViteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "前端资源服务不可用：{e}"),
            Self::Io { path, source } => {
                write!(f, "前端资源读取失败 [{}]：{source}", path.display())
            }
            Self::ManifestNotFound(path) => write!(f, "找不到前端资源清单 [{}]", path.display()),
            Self::InvalidManifest { path, source } => {
                write!(f, "前端资源清单格式无效 [{}]：{source}", path.display())
            }
            Self::MissingChunk(name) => write!(f, "前端资源清单中没有此入口：{name}"),
            Self::InvalidChunk(name) => write!(f, "前端资源块格式无效：{name}"),
            Self::InvalidAttribute(name) => write!(f, "资源标签属性必须是标量：{name}"),
            Self::DisabledPrefetchAttributes => write!(f, "资源预取不能展开被禁用的预加载属性"),
            Self::InvalidPrefetchStrategy(strategy) => write!(f, "资源预取策略无效：{strategy}"),
            Self::InvalidFonts(message) => write!(f, "字体清单无效：{message}"),
            Self::Callback(e) => write!(f, "前端资源回调失败：{e}"),
            Self::Url(e) => write!(f, "前端资源URL生成失败：{e}"),
        }
    }
}
impl Error for ViteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::InvalidManifest { source, .. } => Some(source),
            Self::Url(e) => Some(e),
            Self::Callback(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}
impl From<BindingResolutionError> for ViteError {
    fn from(e: BindingResolutionError) -> Self {
        Self::Unavailable(e)
    }
}
impl From<crate::routing::UrlError> for ViteError {
    fn from(e: crate::routing::UrlError) -> Self {
        Self::Url(e)
    }
}
impl ViteError {
    pub fn callback(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Callback(Box::new(error))
    }
}
fn io_error(path: &Path, source: std::io::Error) -> ViteError {
    ViteError::Io {
        path: path.to_owned(),
        source,
    }
}

/// 属性回调接收真实入口、URL、资源块及整份清单；开发模式后两者为空。
pub struct TagContext<'a> {
    pub source: Option<&'a str>,
    pub url: &'a str,
    pub chunk: Option<&'a Value>,
    pub manifest: Option<&'a Manifest>,
}
type AttributeResolver = dyn Fn(&TagContext<'_>) -> ViteResult<Attributes> + Send + Sync;
type PreloadResolver = dyn Fn(&TagContext<'_>) -> ViteResult<Option<Attributes>> + Send + Sync;
type AssetResolver = dyn Fn(&str, Option<bool>) -> ViteResult<String> + Send + Sync;

#[derive(Clone)]
struct Settings {
    nonce: Option<String>,
    integrity_key: Option<String>,
    entry_points: Vec<String>,
    hot_file: Option<PathBuf>,
    build_directory: String,
    manifest_filename: String,
    fonts_manifest_filename: String,
    asset_resolver: Option<Arc<AssetResolver>>,
    script_attributes: Vec<Arc<AttributeResolver>>,
    style_attributes: Vec<Arc<AttributeResolver>>,
    preload_attributes: Vec<Arc<PreloadResolver>>,
    prefetch_strategy: Option<String>,
    prefetch_concurrency: i64,
    prefetch_event: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            nonce: None,
            integrity_key: Some("integrity".into()),
            entry_points: vec![],
            hot_file: None,
            build_directory: "build".into(),
            manifest_filename: "manifest.json".into(),
            fonts_manifest_filename: "fonts-manifest.json".into(),
            asset_resolver: None,
            script_attributes: vec![],
            style_attributes: vec![],
            preload_attributes: vec![],
            prefetch_strategy: None,
            prefetch_concurrency: 3,
            prefetch_event: "load".into(),
        }
    }
}
#[derive(Default)]
pub(super) struct Execution {
    settings: RwLock<Settings>,
    preloaded: Mutex<IndexMap<String, Vec<String>>>,
}
impl Execution {
    pub(super) fn snapshot(&self) -> Self {
        Self {
            settings: RwLock::new(
                self.settings
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
            ),
            preloaded: Mutex::default(),
        }
    }
}

/// 应用级配置与清单缓存，HTTP／命令／任务作用域分别持有执行期状态。
pub struct Vite {
    owner: Weak<Container>,
    defaults: Arc<Execution>,
    manifests: RwLock<HashMap<PathBuf, Arc<Manifest>>>,
    font_manifests: RwLock<HashMap<PathBuf, Arc<Value>>>,
}
impl Vite {
    pub(super) fn new(owner: Weak<Container>) -> Self {
        Self {
            owner,
            defaults: Arc::default(),
            manifests: RwLock::default(),
            font_manifests: RwLock::default(),
        }
    }
    fn owner(&self) -> ViteResult<Arc<Container>> {
        self.owner
            .upgrade()
            .ok_or_else(|| BindingResolutionError::FacadeNotSet.into())
    }
    fn state(&self) -> ViteResult<Arc<Execution>> {
        let owner = self.owner()?;
        let booted = owner.make::<Application>()?.is_booted();
        if booted && owner.has_active_scope() {
            Ok(owner.make::<Execution>()?)
        } else {
            Ok(self.defaults.clone())
        }
    }
    fn settings(&self) -> ViteResult<Settings> {
        Ok(self
            .state()?
            .settings
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }
    fn update(&self, update: impl FnOnce(&mut Settings)) -> ViteResult<&Self> {
        let state = self.state()?;
        // 回调和被替换的捕获值在锁外销毁，避免析构重入配置锁。
        let mut settings = state
            .settings
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let retired = settings.clone();
        update(&mut settings);
        drop(settings);
        drop(retired);
        Ok(self)
    }
    fn public_path(&self, path: &str) -> ViteResult<PathBuf> {
        Ok(super::paths::join(
            &self.owner()?.make::<Application>()?.public_path(),
            path,
        ))
    }
    pub fn csp_nonce(&self) -> ViteResult<Option<String>> {
        Ok(self.settings()?.nonce)
    }
    pub fn use_csp_nonce(&self, nonce: Option<&str>) -> ViteResult<String> {
        let nonce = nonce.map(str::to_owned).unwrap_or_else(|| Str::random(40));
        self.update(|s| s.nonce = Some(nonce.clone()))?;
        Ok(nonce)
    }
    pub fn use_integrity_key(&self, key: Option<&str>) -> ViteResult<&Self> {
        self.update(|s| s.integrity_key = key.map(str::to_owned))
    }
    pub fn with_entry_points<I, S>(&self, entries: I) -> ViteResult<&Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let entries = entries.into_iter().map(Into::into).collect();
        self.update(|s| s.entry_points = entries)
    }
    pub fn merge_entry_points<I, S>(&self, entries: I) -> ViteResult<&Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let entries: Vec<String> = entries.into_iter().map(Into::into).collect();
        self.update(|s| {
            for entry in entries {
                if !s.entry_points.contains(&entry) {
                    s.entry_points.push(entry);
                }
            }
        })
    }
    pub fn use_manifest_filename(&self, name: impl Into<String>) -> ViteResult<&Self> {
        {
            let name = name.into();
            self.update(|s| s.manifest_filename = name)
        }
    }
    pub fn use_fonts_manifest_filename(&self, name: impl Into<String>) -> ViteResult<&Self> {
        let name = name.into();
        self.update(|settings| settings.fonts_manifest_filename = name)
    }
    pub fn use_build_directory(&self, path: impl Into<String>) -> ViteResult<&Self> {
        {
            let path = path.into();
            self.update(|s| s.build_directory = path)
        }
    }
    pub fn use_hot_file(&self, path: impl Into<PathBuf>) -> ViteResult<&Self> {
        {
            let path = path.into();
            self.update(|s| s.hot_file = Some(path))
        }
    }
    pub fn create_asset_paths_using(
        &self,
        resolver: impl Fn(&str, Option<bool>) -> ViteResult<String> + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.update(|s| s.asset_resolver = Some(Arc::new(resolver)))
    }
    pub fn clear_asset_path_resolver(&self) -> ViteResult<&Self> {
        self.update(|s| s.asset_resolver = None)
    }
    pub fn use_script_tag_attributes(&self, attributes: Attributes) -> ViteResult<&Self> {
        self.use_script_tag_attributes_using(move |_| attributes.clone())
    }
    pub fn use_script_tag_attributes_using(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> Attributes + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.use_script_tag_attributes_using_fallible(move |context| Ok(resolver(context)))
    }
    pub fn use_script_tag_attributes_using_fallible(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> ViteResult<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.update(|s| s.script_attributes.push(Arc::new(resolver)))
    }
    pub fn use_style_tag_attributes(&self, attributes: Attributes) -> ViteResult<&Self> {
        self.use_style_tag_attributes_using(move |_| attributes.clone())
    }
    pub fn use_style_tag_attributes_using(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> Attributes + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.use_style_tag_attributes_using_fallible(move |context| Ok(resolver(context)))
    }
    pub fn use_style_tag_attributes_using_fallible(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> ViteResult<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.update(|s| s.style_attributes.push(Arc::new(resolver)))
    }
    pub fn use_preload_tag_attributes(&self, attributes: Option<Attributes>) -> ViteResult<&Self> {
        self.use_preload_tag_attributes_using(move |_| attributes.clone())
    }
    pub fn use_preload_tag_attributes_using(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> Option<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.use_preload_tag_attributes_using_fallible(move |context| Ok(resolver(context)))
    }
    pub fn use_preload_tag_attributes_using_fallible(
        &self,
        resolver: impl Fn(&TagContext<'_>) -> ViteResult<Option<Attributes>> + Send + Sync + 'static,
    ) -> ViteResult<&Self> {
        self.update(|s| s.preload_attributes.push(Arc::new(resolver)))
    }
    fn hot_path(&self, settings: &Settings) -> ViteResult<PathBuf> {
        settings
            .hot_file
            .clone()
            .map_or_else(|| self.public_path("hot"), Ok)
    }
    pub fn hot_file(&self) -> ViteResult<PathBuf> {
        self.hot_path(&self.settings()?)
    }
    pub fn is_running_hot(&self) -> ViteResult<bool> {
        Ok(self.hot_file()?.is_file())
    }
    fn dev_url(&self, settings: &Settings) -> ViteResult<Option<String>> {
        let path = self.hot_path(settings)?;
        if !path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
        Ok(Some(
            text.trim_end_matches([' ', '\t', '\r', '\n', '\0', '\u{000B}'])
                .to_owned(),
        ))
    }
    pub fn dev_server_url(&self) -> ViteResult<Option<String>> {
        self.dev_url(&self.settings()?)
    }
    fn manifest_path(&self, settings: &Settings, build: &str) -> ViteResult<PathBuf> {
        self.public_path(&format!("{build}/{}", settings.manifest_filename))
    }
    fn manifest(&self, settings: &Settings, build: &str) -> ViteResult<Arc<Manifest>> {
        let path = self.manifest_path(settings, build)?;
        if let Some(cached) = self
            .manifests
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
        {
            return Ok(cached.clone());
        }
        if !path.is_file() {
            return Err(ViteError::ManifestNotFound(path));
        }
        let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
        let manifest = Arc::new(serde_json::from_slice(&bytes).map_err(|source| {
            ViteError::InvalidManifest {
                path: path.clone(),
                source,
            }
        })?);
        let mut cache = self
            .manifests
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(cache.entry(path).or_insert(manifest).clone())
    }
    fn chunk<'a>(&self, manifest: &'a Manifest, name: &str) -> ViteResult<&'a Value> {
        manifest
            .get(name)
            .ok_or_else(|| ViteError::MissingChunk(name.into()))
    }
    fn file<'a>(&self, chunk: &'a Value, name: &str) -> ViteResult<&'a str> {
        chunk
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| ViteError::InvalidChunk(name.into()))
    }
    fn asset_path(&self, path: &str) -> ViteResult<String> {
        if let Some(resolver) = &self.settings()?.asset_resolver {
            return resolver(path, None);
        }
        let owner = self.owner()?;
        let mut urls = owner.get::<UrlGenerator>("url")?.as_ref().clone();
        if crate::support::facades::current_container()
            .is_some_and(|current| Arc::ptr_eq(&current, &owner))
        {
            urls = urls
                .with_uri(crate::http::request_context::with_url_uri())
                .with_base_url(crate::http::request_context::base_url().unwrap_or_default());
        }
        Ok(urls.asset(path, None)?)
    }
    pub fn asset(&self, asset: &str, build: Option<&str>) -> ViteResult<String> {
        let settings = self.settings()?;
        if let Some(url) = self.dev_url(&settings)? {
            return Ok(format!("{url}/{asset}"));
        }
        let build = build.unwrap_or(&settings.build_directory);
        let manifest = self.manifest(&settings, build)?;
        let file = self.file(self.chunk(&manifest, asset)?, asset)?;
        self.asset_path(&format!("{build}/{file}"))
    }
    pub fn content(&self, asset: &str, build: Option<&str>) -> ViteResult<Vec<u8>> {
        let settings = self.settings()?;
        let build = build.unwrap_or(&settings.build_directory);
        let manifest = self.manifest(&settings, build)?;
        let path = self.public_path(&format!(
            "{build}/{}",
            self.file(self.chunk(&manifest, asset)?, asset)?
        ))?;
        fs::read(&path).map_err(|e| io_error(&path, e))
    }
    pub fn manifest_hash(&self, build: Option<&str>) -> ViteResult<Option<String>> {
        use md5::{Digest, Md5};
        let settings = self.settings()?;
        if self.hot_path(&settings)?.is_file() {
            return Ok(None);
        }
        let path = self.manifest_path(&settings, build.unwrap_or(&settings.build_directory))?;
        if !path.is_file() {
            return Ok(None);
        }
        let digest = Md5::digest(fs::read(&path).map_err(|e| io_error(&path, e))?);
        let text = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Some(text))
    }
    pub fn preloaded_assets(&self) -> ViteResult<IndexMap<String, Vec<String>>> {
        Ok(self
            .state()?
            .preloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }
    pub fn flush(&self) -> ViteResult<()> {
        self.font_manifests
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.state()?
            .preloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        Ok(())
    }
    pub fn to_html(&self) -> ViteResult<String> {
        let entries = self.settings()?.entry_points;
        Ok(self.render(&entries, None)?.to_html().to_owned())
    }
    pub fn react_refresh(&self) -> ViteResult<Option<HtmlString>> {
        let settings = self.settings()?;
        let Some(url) = self.dev_url(&settings)? else {
            return Ok(None);
        };
        let attributes = render::parse_attributes(
            &[(
                "nonce".into(),
                settings.nonce.map(Value::String).unwrap_or(Value::Null),
            )]
            .into(),
        )?
        .join(" ");
        Ok(Some(HtmlString::new(format!(
            "<script type=\"module\" {attributes}>\n    import RefreshRuntime from '{url}/@react-refresh'\n    RefreshRuntime.injectIntoGlobalHook(window)\n    window.$RefreshReg$ = () => {{}}\n    window.$RefreshSig$ = () => (type) => type\n    window.__vite_plugin_react_preamble_installed__ = true\n</script>"
        ))))
    }
}
