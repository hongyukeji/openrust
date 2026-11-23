//! 保留原文格式的环境文件编辑；文件变更不隐式更新应用快照或运行中服务。

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, io,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

mod document;
mod storage;

/// 文件编辑失败原因。Debug 和 Display 均不包含变量值或源文件内容。
#[derive(Debug)]
#[non_exhaustive]
pub enum EnvironmentFileError {
    InvalidPath,
    Unbound,
    InvalidKey {
        key: String,
    },
    InvalidValue {
        key: String,
    },
    InvalidDocument,
    DuplicateKey {
        key: String,
    },
    Conflict,
    Busy,
    TooLarge,
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    DurabilityUncertain,
    ReplacementUncertain {
        recovery: PathBuf,
    },
}
impl fmt::Display for EnvironmentFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => f.write_str("环境文件必须是普通文件，不能是符号链接"),
            Self::Unbound => {
                f.write_str("应用尚未绑定实际环境文件，请先完成应用引导或显式指定路径")
            }
            Self::InvalidKey { key } => write!(f, "无效的环境变量名：{key}"),
            Self::InvalidValue { key } => write!(f, "环境变量 {key} 的值无法安全写入"),
            Self::InvalidDocument => f.write_str("环境文件语法无效，未执行写入"),
            Self::DuplicateKey { key } => write!(f, "环境变量 {key} 存在重复定义，未执行写入"),
            Self::Conflict => f.write_str("环境文件已被其他操作修改，请重新读取后再保存"),
            Self::Busy => f.write_str("环境文件正由其他写入操作占用，请稍后重试"),
            Self::TooLarge => f.write_str("环境文件超过 8 MiB 编辑上限"),
            Self::Io { operation, kind } => write!(f, "环境文件{operation}失败：{kind}"),
            Self::ReplacementUncertain { recovery } => write!(
                f,
                "环境文件替换状态不确定，请核查目标；恢复文件保留于 {}",
                recovery.display()
            ),
            Self::DurabilityUncertain => {
                f.write_str("环境文件已替换，但目录同步失败，请核查落盘状态")
            }
        }
    }
}
impl std::error::Error for EnvironmentFileError {}
pub(super) fn io_error(operation: &'static str, error: io::Error) -> EnvironmentFileError {
    EnvironmentFileError::Io {
        operation,
        kind: error.kind(),
    }
}

/// 指向确定路径的环境文件，不持有进程环境或运行时配置的可变引用。
#[derive(Clone, Debug)]
pub struct EnvironmentFile {
    path: PathBuf,
}

impl EnvironmentFile {
    /// 打开已有文件；相对路径以当前目录解析，目标符号链接会被拒绝。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EnvironmentFileError> {
        let path = storage::normalize(path.as_ref())?;
        storage::read(&path)?;
        Ok(Self { path })
    }

    /// 显式创建环境文件；已有目标不会覆盖，新文件默认仅供所有者读写。
    pub fn create(path: impl AsRef<Path>, content: &str) -> Result<Self, EnvironmentFileError> {
        document::validate(content)?;
        let path = storage::normalize(path.as_ref())?;
        let _lock = storage::lock(&path)?;
        storage::create(&path, content)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取文件解析值，引用展开与现有 dotenvy 加载器一致，不覆盖为进程同名值。
    pub fn get(&self, key: &str) -> Result<Option<String>, EnvironmentFileError> {
        self.edit()?.get(key)
    }
    /// 读取包含引号的原始值 token，不展开变量引用。
    pub fn get_raw(&self, key: &str) -> Result<Option<String>, EnvironmentFileError> {
        self.edit()?.get_raw(key)
    }
    pub fn contains(&self, key: &str) -> Result<bool, EnvironmentFileError> {
        self.edit()?.contains(key)
    }
    pub fn all(&self) -> Result<BTreeMap<String, String>, EnvironmentFileError> {
        self.edit()?.all()
    }

    /// 创建独立编辑快照；不会长期持锁，保存时检测过期快照。
    pub fn edit(&self) -> Result<EnvironmentEditor, EnvironmentFileError> {
        let original = storage::read(&self.path)?;
        document::validate(&original)?;
        Ok(EnvironmentEditor {
            file: self.clone(),
            text: original.clone(),
            restrictive: false,
            original,
        })
    }

    fn write(
        &self,
        edit: impl FnOnce(&mut EnvironmentEditor) -> Result<(), EnvironmentFileError>,
    ) -> Result<bool, EnvironmentFileError> {
        let lock = storage::lock(&self.path)?;
        let mut editor = self.edit()?;
        edit(&mut editor)?;
        editor.save_locked(&lock)
    }
    pub fn set(&self, key: &str, value: impl AsRef<str>) -> Result<bool, EnvironmentFileError> {
        self.write(|editor| {
            editor.set(key, value)?;
            Ok(())
        })
    }
    pub fn set_expression(
        &self,
        key: &str,
        expression: impl AsRef<str>,
    ) -> Result<bool, EnvironmentFileError> {
        self.write(|editor| {
            editor.set_expression(key, expression)?;
            Ok(())
        })
    }
    pub fn set_many<K: AsRef<str>, V: AsRef<str>>(
        &self,
        values: impl IntoIterator<Item = (K, V)>,
    ) -> Result<bool, EnvironmentFileError> {
        self.write(|editor| {
            editor.set_many(values)?;
            Ok(())
        })
    }
    pub fn remove(&self, key: &str) -> Result<bool, EnvironmentFileError> {
        self.write(|editor| {
            editor.remove(key)?;
            Ok(())
        })
    }
}

/// 独立编辑会话。仅 save 写文件；丢弃会话不会写入，也不会记录明文内容。
pub struct EnvironmentEditor {
    file: EnvironmentFile,
    original: Zeroizing<String>,
    text: Zeroizing<String>,
    restrictive: bool,
}
impl fmt::Debug for EnvironmentEditor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentEditor")
            .field("path", &self.file.path)
            .field("changed", &self.has_changes())
            .finish_non_exhaustive()
    }
}
impl EnvironmentEditor {
    pub fn get(&self, key: &str) -> Result<Option<String>, EnvironmentFileError> {
        document::unique(&self.text, key)?;
        Ok(document::values(&self.text)?.remove(key))
    }
    pub fn get_raw(&self, key: &str) -> Result<Option<String>, EnvironmentFileError> {
        document::raw(&self.text, key)
    }
    pub fn contains(&self, key: &str) -> Result<bool, EnvironmentFileError> {
        Ok(document::unique(&self.text, key)?.is_some())
    }
    pub fn all(&self) -> Result<BTreeMap<String, String>, EnvironmentFileError> {
        document::values(&self.text)
    }
    pub fn has_changes(&self) -> bool {
        self.text.as_str() != self.original.as_str()
    }
    /// 显式取得待保存正文；正文可能含凭据，不应直接记录到日志。
    pub fn content(&self) -> &str {
        &self.text
    }
    pub fn set(
        &mut self,
        key: &str,
        value: impl AsRef<str>,
    ) -> Result<&mut Self, EnvironmentFileError> {
        self.text = document::set(&self.text, key, value.as_ref(), false)?;
        Ok(self)
    }
    /// 只启用现有加载器的变量展开，不执行 shell 命令。
    pub fn set_expression(
        &mut self,
        key: &str,
        value: impl AsRef<str>,
    ) -> Result<&mut Self, EnvironmentFileError> {
        self.text = document::set(&self.text, key, value.as_ref(), true)?;
        Ok(self)
    }
    pub fn set_many<K: AsRef<str>, V: AsRef<str>>(
        &mut self,
        values: impl IntoIterator<Item = (K, V)>,
    ) -> Result<&mut Self, EnvironmentFileError> {
        let mut text = self.text.clone();
        let mut keys = BTreeSet::new();
        for (key, value) in values {
            let key = key.as_ref();
            if !keys.insert(key.to_owned()) {
                return Err(EnvironmentFileError::DuplicateKey { key: key.into() });
            }
            text = document::set(&text, key, value.as_ref(), false)?;
        }
        self.text = text;
        Ok(self)
    }
    pub fn remove(&mut self, key: &str) -> Result<&mut Self, EnvironmentFileError> {
        self.text = document::remove(&self.text, key)?;
        Ok(self)
    }
    /// 原子保存当前会话。成功后可继续编辑；未改变内容时不更新文件修改时间。
    pub fn save(&mut self) -> Result<bool, EnvironmentFileError> {
        let lock = storage::lock(&self.file.path)?;
        self.save_locked(&lock)
    }
    fn save_locked(&mut self, _lock: &storage::Lock) -> Result<bool, EnvironmentFileError> {
        let current = storage::read(&self.file.path)?;
        if current.as_str() != self.original.as_str() {
            return Err(EnvironmentFileError::Conflict);
        }
        document::validate(&self.text)?;
        if !self.has_changes() {
            return Ok(false);
        }
        storage::replace(
            &self.file.path,
            &self.original,
            &self.text,
            self.restrictive,
        )?;
        self.original = self.text.clone();
        Ok(true)
    }
    /// 命令消费者完成自身严格校验后，共用同一文件事务提交层。
    pub(crate) fn replace_content(&mut self, content: String) -> Result<(), EnvironmentFileError> {
        document::validate(&content)?;
        self.text = Zeroizing::new(content);
        Ok(())
    }
    pub(crate) fn restrict_permissions(&mut self) {
        self.restrictive = true;
    }

    pub(crate) fn original(&self) -> &str {
        &self.original
    }
}

// 安装器共用文本编辑后端，仍由安装器决定哪些项目需要保留表达式。
pub(crate) fn render_environment<'a>(
    source: &str,
    values: impl IntoIterator<Item = (&'a str, &'a str, bool)>,
) -> Result<String, EnvironmentFileError> {
    document::validate(source)?;
    let mut text = Zeroizing::new(source.to_owned());
    for (key, value, expression) in values {
        text = document::set(&text, key, value, expression)?;
    }
    Ok(text.to_string())
}
