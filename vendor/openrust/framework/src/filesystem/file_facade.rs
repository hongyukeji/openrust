use super::{DirectoryDepth, FileInfo, FileMacro, FileMode, FileResult, Filesystem};
use crate::{container::ServiceKey, support::facades::Facade};
use std::{
    any::Any,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// 本地文件服务门面，与 Storage 的具名磁盘接口区分。
pub struct File;
impl Facade for File {
    type Service = Filesystem;
    fn get_facade_accessor() -> ServiceKey {
        "files".into()
    }
}
macro_rules! file_queries {
    ($(fn $name:ident($($argument:ident:$argument_type:ty),*)->$output:ty;)+)=>{
        impl File {$(pub fn $name($($argument:$argument_type),*)->FileResult<$output>{Ok(Self::get_facade_root()?.$name($($argument),*))})+}
    };
}
macro_rules! file_operations {
    ($(fn $name:ident($($argument:ident:$argument_type:ty),*)->$output:ty;)+)=>{
        impl File {$(pub fn $name($($argument:$argument_type),*)->FileResult<$output>{Self::get_facade_root()?.$name($($argument),*)})+}
    };
}
file_queries! {
    fn exists(path:impl AsRef<Path>)->bool;
    fn missing(path:impl AsRef<Path>)->bool;
    fn is_file(path:impl AsRef<Path>)->bool;
    fn is_directory(path:impl AsRef<Path>)->bool;
    fn is_readable(path:impl AsRef<Path>)->bool;
    fn is_writable(path:impl AsRef<Path>)->bool;
    fn delete(path:impl AsRef<Path>)->bool;
    fn has_same_hash(first:impl AsRef<Path>,second:impl AsRef<Path>)->bool;
    fn basename(path:impl AsRef<Path>)->OsString;
    fn dirname(path:impl AsRef<Path>)->PathBuf;
    fn name(path:impl AsRef<Path>)->OsString;
    fn extension(path:impl AsRef<Path>)->OsString;
}
file_operations! {
    fn move_directory(source:impl AsRef<Path>,target:impl AsRef<Path>)->bool;
    fn move_directory_with_overwrite(source:impl AsRef<Path>,target:impl AsRef<Path>,overwrite:bool)->bool;
    fn delete_directory(directory:impl AsRef<Path>)->bool;
    fn delete_directory_preserving_root(directory:impl AsRef<Path>,preserve:bool)->bool;
    fn clean_directory(directory:impl AsRef<Path>)->bool;

    fn files(directory:impl AsRef<Path>)->Vec<FileInfo>;
    fn files_with_options(directory:impl AsRef<Path>,hidden:bool,depth:DirectoryDepth)->Vec<FileInfo>;
    fn all_files(directory:impl AsRef<Path>)->Vec<FileInfo>;
    fn all_files_with_hidden(directory:impl AsRef<Path>,hidden:bool)->Vec<FileInfo>;
    fn directories(directory:impl AsRef<Path>)->Vec<PathBuf>;
    fn directories_with_depth(directory:impl AsRef<Path>,depth:DirectoryDepth)->Vec<PathBuf>;
    fn all_directories(directory:impl AsRef<Path>)->Vec<PathBuf>;
    fn is_empty_directory(directory:impl AsRef<Path>)->bool;
    fn is_empty_directory_ignoring_dot_files(directory:impl AsRef<Path>,ignore_dot_files:bool)->bool;
    fn ensure_directory_exists(path:impl AsRef<Path>)->();
    fn ensure_directory_exists_with_options(path:impl AsRef<Path>,mode:u32,recursive:bool)->();
    fn make_directory(path:impl AsRef<Path>)->bool;
    fn make_directory_with_options(path:impl AsRef<Path>,mode:u32,recursive:bool,force:bool)->bool;
    fn copy_directory(source:impl AsRef<Path>,target:impl AsRef<Path>)->bool;
    fn delete_directories(directory:impl AsRef<Path>)->bool;
    fn json(path:impl AsRef<Path>)->serde_json::Value;
    fn json_strict(path:impl AsRef<Path>)->serde_json::Value;
    fn json_with_options(path:impl AsRef<Path>,lock:bool,strict:bool)->serde_json::Value;
    fn lines(path:impl AsRef<Path>)->crate::support::LazyCollection<'static,Vec<u8>>;
    fn mime_type(path:impl AsRef<Path>)->String;
    fn guess_extension(path:impl AsRef<Path>)->Option<String>;
    fn link(target:impl AsRef<Path>,link:impl AsRef<Path>)->();
    fn relative_link(target:impl AsRef<Path>,link:impl AsRef<Path>)->();
    fn glob(pattern:&str)->Vec<PathBuf>;
    fn get(path:impl AsRef<Path>)->Vec<u8>;
    fn get_with_lock(path:impl AsRef<Path>,lock:bool)->Vec<u8>;
    fn shared_get(path:impl AsRef<Path>)->Vec<u8>;
    fn get_text(path:impl AsRef<Path>)->String;
    fn put(path:impl AsRef<Path>,content:impl AsRef<[u8]>)->usize;
    fn put_with_lock(path:impl AsRef<Path>,content:impl AsRef<[u8]>,lock:bool)->usize;
    fn append(path:impl AsRef<Path>,content:impl AsRef<[u8]>)->usize;
    fn append_with_lock(path:impl AsRef<Path>,content:impl AsRef<[u8]>,lock:bool)->usize;
    fn prepend(path:impl AsRef<Path>,content:impl AsRef<[u8]>)->usize;
    fn replace(path:impl AsRef<Path>,content:impl AsRef<[u8]>)->();
    fn replace_with_mode(path:impl AsRef<Path>,content:impl AsRef<[u8]>,mode:Option<u32>)->();
    fn replace_in_file(search:impl AsRef<[u8]>,replacement:impl AsRef<[u8]>,path:impl AsRef<Path>)->();
    fn replace_many_in_file(replacements:&[(&[u8],&[u8])],path:impl AsRef<Path>)->();
    fn r#move(source:impl AsRef<Path>,target:impl AsRef<Path>)->();
    fn copy(source:impl AsRef<Path>,target:impl AsRef<Path>)->();
    fn size(path:impl AsRef<Path>)->u64;
    fn last_modified(path:impl AsRef<Path>)->i64;
    fn mode(path:impl AsRef<Path>)->u32;
    fn chmod(path:impl AsRef<Path>,mode:Option<u32>)->FileMode;
    fn set_mode(path:impl AsRef<Path>,mode:u32)->();
    fn hash(path:impl AsRef<Path>)->String;
    fn hash_with_algorithm(path:impl AsRef<Path>,algorithm:&str)->String;
    fn r#type(path:impl AsRef<Path>)->&'static str;
}
impl File {
    pub fn register_module<A: Any + Send + Sync, T: Any + Send + Sync>(
        path: impl AsRef<Path>,
        callback: impl Fn(&Filesystem, A) -> FileResult<T> + Send + Sync + 'static,
    ) -> FileResult<()> {
        Self::get_facade_root()?.register_module(path, callback)
    }
    pub fn get_require<T: Any + Send + Sync, A: Any + Send + Sync>(
        path: impl AsRef<Path>,
        data: A,
    ) -> FileResult<T> {
        Self::get_facade_root()?.get_require(path, data)
    }
    pub fn require_once<T: Any + Send + Sync, A: Any + Send + Sync>(
        path: impl AsRef<Path>,
        data: A,
    ) -> FileResult<super::RequiredFile<T>> {
        Self::get_facade_root()?.require_once(path, data)
    }
    pub fn delete_many<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) -> FileResult<bool> {
        Ok(Self::get_facade_root()?.delete_many(paths))
    }
    pub fn r#macro<A: Any + Send + Sync, T: Any + Send + Sync>(
        name: impl Into<String>,
        callback: impl Fn(&Filesystem, A) -> FileResult<T> + Send + Sync + 'static,
    ) -> FileResult<()> {
        Self::get_facade_root()?.r#macro(name, callback);
        Ok(())
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, FileMacro)>,
        replace: bool,
    ) -> FileResult<()> {
        Self::get_facade_root()?.mixin(macros, replace);
        Ok(())
    }
    pub fn has_macro(name: &str) -> FileResult<bool> {
        Ok(Self::get_facade_root()?.has_macro(name))
    }
    pub fn flush_macros() -> FileResult<()> {
        Self::get_facade_root()?.flush_macros();
        Ok(())
    }
    pub fn macro_call<T: Any + Send + Sync, A: Any + Send + Sync>(
        name: &str,
        arguments: A,
    ) -> FileResult<T> {
        Self::get_facade_root()?.macro_call(name, arguments)
    }
}

macro_rules! conditional_facade {
    ($($name:ident);+ $(;)?)=> {impl File {$(
        pub fn $name<V:super::ConditionValue,T>(value:V,callback:impl FnOnce(&Filesystem,V)->FileResult<Option<T>>)->FileResult<super::ConditionalResult<std::sync::Arc<Filesystem>,T>> {
            let root=Self::get_facade_root()?;
            let result=root.$name(value,callback)?;
            Ok(result.map_target(|_|root.clone()))
        }
    )+}}
}
conditional_facade! {when;unless}
macro_rules! conditional_else_facade {
    ($($name:ident);+ $(;)?)=> {impl File {$(
        pub fn $name<V:super::ConditionValue,T>(value:V,callback:impl FnOnce(&Filesystem,V)->FileResult<Option<T>>,fallback:impl FnOnce(&Filesystem,V)->FileResult<Option<T>>)->FileResult<super::ConditionalResult<std::sync::Arc<Filesystem>,T>> {
            let root=Self::get_facade_root()?;
            let result=root.$name(value,callback,fallback)?;
            Ok(result.map_target(|_|root.clone()))
        }
    )+}}
}
conditional_else_facade! {when_else;unless_else}
macro_rules! conditional_test_facade {
    ($($name:ident);+ $(;)?)=> {impl File {$(
        pub fn $name<V:super::ConditionValue,T>(test:impl FnOnce(&Filesystem)->V,callback:impl FnOnce(&Filesystem,V)->FileResult<Option<T>>)->FileResult<super::ConditionalResult<std::sync::Arc<Filesystem>,T>> {
            let root=Self::get_facade_root()?;
            let result=root.$name(test,callback)?;
            Ok(result.map_target(|_|root.clone()))
        }
    )+}}
}
conditional_test_facade! {when_test;unless_test}
impl File {
    pub fn when_condition(
        value: impl super::ConditionValue,
    ) -> FileResult<super::HigherOrderWhenProxy<std::sync::Arc<Filesystem>>> {
        Ok(super::HigherOrderWhenProxy::new(
            Self::get_facade_root()?,
            Some(super::ConditionValue::is_truthy(&value)),
            false,
        ))
    }
    pub fn unless_condition(
        value: impl super::ConditionValue,
    ) -> FileResult<super::HigherOrderWhenProxy<std::sync::Arc<Filesystem>>> {
        Ok(super::HigherOrderWhenProxy::new(
            Self::get_facade_root()?,
            Some(!super::ConditionValue::is_truthy(&value)),
            false,
        ))
    }
    pub fn when_capture() -> FileResult<super::HigherOrderWhenProxy<std::sync::Arc<Filesystem>>> {
        Ok(super::HigherOrderWhenProxy::new(
            Self::get_facade_root()?,
            None,
            false,
        ))
    }
    pub fn unless_capture() -> FileResult<super::HigherOrderWhenProxy<std::sync::Arc<Filesystem>>> {
        Ok(super::HigherOrderWhenProxy::new(
            Self::get_facade_root()?,
            None,
            true,
        ))
    }
}
