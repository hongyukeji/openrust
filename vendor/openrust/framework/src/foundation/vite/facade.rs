use super::{Attributes, TagContext, Vite, ViteResult};
use crate::{
    container::ServiceKey,
    support::{HtmlString, facades::Facade},
};
use std::{path::PathBuf, sync::Arc};

/// 前端资源服务的公开静态入口。
pub struct ViteFacade;
impl Facade for ViteFacade {
    type Service = Vite;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Vite>()
    }
}
macro_rules! queries {
    ($(fn $name:ident($($arg:ident:$ty:ty),*)->$result:ty;)+)=>{impl ViteFacade {$(
        pub fn $name($($arg:$ty),*)->ViteResult<$result> {Self::get_facade_root()?.$name($($arg),*)}
    )+}}
}
queries! {
    fn csp_nonce()->Option<String>;
    fn use_csp_nonce(nonce:Option<&str>)->String;
    fn hot_file()->PathBuf;
    fn is_running_hot()->bool;
    fn dev_server_url()->Option<String>;
    fn asset(asset:&str,build:Option<&str>)->String;
    fn content(asset:&str,build:Option<&str>)->Vec<u8>;
    fn manifest_hash(build:Option<&str>)->Option<String>;
    fn preloaded_assets()->indexmap::IndexMap<String,Vec<String>>;
    fn to_html()->String;
    fn react_refresh()->Option<HtmlString>;
    fn flush()->();
    fn fonts(aliases:Option<&[&str]>)->HtmlString;
}
macro_rules! setters {
    ($(fn $name:ident($($arg:ident:$ty:ty),*);)+)=>{impl ViteFacade {$(
        pub fn $name($($arg:$ty),*)->ViteResult<Arc<Vite>> {let service=Self::get_facade_root()?;service.$name($($arg),*)?;Ok(service)}
    )+}}
}
setters! {
    fn use_integrity_key(key:Option<&str>);
    fn use_manifest_filename(filename:impl Into<String>);
    fn use_build_directory(directory:impl Into<String>);
    fn use_fonts_manifest_filename(filename:impl Into<String>);
    fn use_hot_file(path:impl Into<PathBuf>);
    fn use_script_tag_attributes(attributes:Attributes);
    fn use_style_tag_attributes(attributes:Attributes);
    fn use_preload_tag_attributes(attributes:Option<Attributes>);
    fn clear_asset_path_resolver();
    fn prefetch(concurrency:Option<i64>,event:Option<&str>);
    fn use_waterfall_prefetching(concurrency:Option<i64>);
    fn use_aggressive_prefetching();
    fn use_prefetch_strategy(strategy:Option<&str>,concurrency:Option<i64>);
}
impl ViteFacade {
    pub fn with_entry_points<I, S>(entries: I) -> ViteResult<Arc<Vite>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let root = Self::get_facade_root()?;
        root.with_entry_points(entries)?;
        Ok(root)
    }
    pub fn merge_entry_points<I, S>(entries: I) -> ViteResult<Arc<Vite>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let root = Self::get_facade_root()?;
        root.merge_entry_points(entries)?;
        Ok(root)
    }
    pub fn render<I, S>(entries: I, build: Option<&str>) -> ViteResult<HtmlString>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::get_facade_root()?.render(entries, build)
    }
    pub fn create_asset_paths_using(
        callback: impl Fn(&str, Option<bool>) -> ViteResult<String> + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.create_asset_paths_using(callback)?;
        Ok(root)
    }
    pub fn use_script_tag_attributes_using(
        callback: impl Fn(&TagContext<'_>) -> Attributes + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_script_tag_attributes_using(callback)?;
        Ok(root)
    }
    pub fn use_script_tag_attributes_using_fallible(
        callback: impl Fn(&TagContext<'_>) -> ViteResult<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_script_tag_attributes_using_fallible(callback)?;
        Ok(root)
    }
    pub fn use_style_tag_attributes_using(
        callback: impl Fn(&TagContext<'_>) -> Attributes + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_style_tag_attributes_using(callback)?;
        Ok(root)
    }
    pub fn use_style_tag_attributes_using_fallible(
        callback: impl Fn(&TagContext<'_>) -> ViteResult<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_style_tag_attributes_using_fallible(callback)?;
        Ok(root)
    }
    pub fn use_preload_tag_attributes_using(
        callback: impl Fn(&TagContext<'_>) -> Option<Attributes> + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_preload_tag_attributes_using(callback)?;
        Ok(root)
    }
    pub fn use_preload_tag_attributes_using_fallible(
        callback: impl Fn(&TagContext<'_>) -> ViteResult<Option<Attributes>> + Send + Sync + 'static,
    ) -> ViteResult<Arc<Vite>> {
        let root = Self::get_facade_root()?;
        root.use_preload_tag_attributes_using_fallible(callback)?;
        Ok(root)
    }
}

impl ViteFacade {
    pub fn r#macro<A: std::any::Any + Send + Sync, T: std::any::Any + Send + Sync>(
        name: impl Into<String>,
        callback: impl Fn(&Vite, A) -> ViteResult<T> + Send + Sync + 'static,
    ) -> ViteResult<()> {
        Self::get_facade_root()?.r#macro(name, callback);
        Ok(())
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, super::ViteMacro)>,
        replace: bool,
    ) -> ViteResult<()> {
        Self::get_facade_root()?.mixin(macros, replace);
        Ok(())
    }
    pub fn mixin_using<K, F>(
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> ViteResult<()>
    where
        K: Into<String>,
        F: FnOnce() -> ViteResult<super::ViteMacro>,
    {
        Self::get_facade_root()?.mixin_using(factories, replace)
    }
    pub fn has_macro(name: &str) -> ViteResult<bool> {
        Ok(Self::get_facade_root()?.has_macro(name))
    }
    pub fn flush_macros() -> ViteResult<()> {
        Self::get_facade_root()?.flush_macros();
        Ok(())
    }
    pub fn macro_call<T: std::any::Any + Send + Sync, A: std::any::Any + Send + Sync>(
        name: &str,
        arguments: A,
    ) -> ViteResult<T> {
        Self::get_facade_root()?.macro_call(name, arguments)
    }
}
