use super::{FileError, FileMacro, FileResult, Filesystem, io_error};
use std::{
    any::Any,
    fs,
    path::{Path, PathBuf},
    sync::PoisonError,
};

/// 首次加载的返回值与已经包含的状态分开，避免与模块返回true混淆。
#[derive(Debug, PartialEq, Eq)]
pub enum RequiredFile<T> {
    Value(T),
    AlreadyIncluded,
}

impl Filesystem {
    /// 注册编译后的Rust模块；不在运行时解释或编译文件内容。
    pub fn register_module<A: Any + Send + Sync, T: Any + Send + Sync>(
        &self,
        path: impl AsRef<Path>,
        callback: impl Fn(&Self, A) -> FileResult<T> + Send + Sync + 'static,
    ) -> FileResult<()> {
        let path = module_path(path.as_ref())?;
        let retired = self
            .modules
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path, FileMacro::new(callback));
        drop(retired);
        Ok(())
    }

    /// 每次执行回调并传入拥有所有权的参数；成功查找后记入包含集合。
    pub fn get_require<T: Any + Send + Sync, A: Any + Send + Sync>(
        &self,
        path: impl AsRef<Path>,
        data: A,
    ) -> FileResult<T> {
        let path = module_path(path.as_ref())?;
        let module = self.module(&path)?;
        self.included
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone());
        module.invoke(self, &path.to_string_lossy(), data)
    }

    /// 在调用前标记已包含，支持递归及失败后的再次加载；回调期间不持锁。
    pub fn require_once<T: Any + Send + Sync, A: Any + Send + Sync>(
        &self,
        path: impl AsRef<Path>,
        data: A,
    ) -> FileResult<RequiredFile<T>> {
        let path = module_path(path.as_ref())?;
        if self
            .included
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&path)
        {
            return Ok(RequiredFile::AlreadyIncluded);
        }
        let module = self.module(&path)?;
        if !self
            .included
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone())
        {
            return Ok(RequiredFile::AlreadyIncluded);
        }
        module
            .invoke(self, &path.to_string_lossy(), data)
            .map(RequiredFile::Value)
    }

    fn module(&self, path: &Path) -> FileResult<FileMacro> {
        self.modules
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .cloned()
            .ok_or_else(|| FileError::UnregisteredModule(path.to_owned()))
    }
}

fn module_path(path: &Path) -> FileResult<PathBuf> {
    if !path.is_file() {
        return Err(io_error(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "路径不是普通文件"),
        ));
    }
    fs::canonicalize(path).map_err(|error| io_error(path, error))
}
