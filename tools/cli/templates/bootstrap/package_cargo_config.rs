//! 读取包发现与构建清单共用的 Cargo 文件配置。

use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeSet,
    env, fs, io,
    path::{self, Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, TableLike, Value};

const MAX_INCLUDE_DEPTH: usize = 64;
const HOME_ENV: [&str; 3] = ["CARGO_HOME", "HOME", "USERPROFILE"];

pub(crate) struct CargoConfig {
    /// 从低到高排列，包含文件早于包含它的文件。
    pub(crate) layers: Vec<ConfigLayer>,
    app: PathBuf,
    watched: BTreeSet<PathBuf>,
}

pub(crate) struct ConfigLayer {
    pub(crate) file: PathBuf,
    pub(crate) document: DocumentMut,
}

impl CargoConfig {
    pub(crate) fn read(app: &Path) -> io::Result<Self> {
        let app = path::absolute(app)
            .map_err(|error| io::Error::new(error.kind(), "无法定位 Cargo 应用目录"))?;
        let roots = app
            .ancestors()
            .map(|directory| directory.join(".cargo"))
            .collect::<Vec<_>>();
        let home = env::var_os("CARGO_HOME")
            .filter(|value| !value.is_empty())
            .map(|value| app.join(value))
            .or_else(|| env::home_dir().map(|home| home.join(".cargo")))
            .ok_or_else(|| io::Error::other("无法定位 Cargo home"))?;
        let real_home = real_path(&home);
        let separate_home = !roots
            .iter()
            .any(|root| root == &home || real_path(root) == real_home);
        let mut config = Self {
            layers: Vec::new(),
            app,
            watched: BTreeSet::new(),
        };
        if separate_home {
            config.read_root(&home)?;
        }
        for root in roots.iter().rev() {
            config.read_root(root)?;
        }
        Ok(config)
    }

    pub(crate) fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"openrust-cargo-config-v1");
        for layer in &self.layers {
            let path = relative_path(&self.app, &layer.file);
            hash_field(&mut hasher, path.as_os_str().as_encoded_bytes());
            hash_field(&mut hasher, layer.document.to_string().as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    pub(crate) fn watch(&self, out: &mut impl io::Write) -> io::Result<()> {
        for env in HOME_ENV {
            writeln!(out, "cargo:rerun-if-env-changed={env}")?;
        }
        for file in &self.watched {
            let path = file
                .to_str()
                .filter(|path| !path.contains(['\n', '\r']))
                .ok_or_else(|| invalid("Cargo 配置路径无法写入构建脚本指令"))?;
            writeln!(out, "cargo:rerun-if-changed={path}")?;
        }
        Ok(())
    }

    fn read_root(&mut self, root: &Path) -> io::Result<()> {
        let mut seen = BTreeSet::new();
        let legacy = root.join("config");
        if !self.read_file(legacy, true, &mut seen, 0)? {
            self.read_file(root.join("config.toml"), true, &mut seen, 0)?;
        }
        Ok(())
    }

    fn read_file(
        &mut self,
        file: PathBuf,
        optional: bool,
        seen: &mut BTreeSet<PathBuf>,
        depth: usize,
    ) -> io::Result<bool> {
        self.watched.insert(file.clone());
        if depth > MAX_INCLUDE_DEPTH {
            return Err(invalid("Cargo 配置 include 嵌套层级超出限制"));
        }
        let contents = match fs::read_to_string(&file) {
            Ok(contents) => contents,
            Err(error) if optional && error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io::Error::new(error.kind(), "无法读取 Cargo 配置文件")),
        };
        // Cargo 对每个顶层文件独立检查 include，重复引用同一个文件也会失败。
        if !seen.insert(real_path(&file)) {
            return Err(invalid("Cargo 配置 include 循环或重复"));
        }
        let document = contents
            .parse::<DocumentMut>()
            .map_err(|_| invalid("无法解析 Cargo 配置文件"))?;
        let directory = file
            .parent()
            .ok_or_else(|| invalid("无法定位 Cargo 配置目录"))?;
        for (include, optional) in includes(&document)? {
            self.read_file(directory.join(include), optional, seen, depth + 1)?;
        }
        self.layers.push(ConfigLayer { file, document });
        Ok(true)
    }
}

fn includes(document: &DocumentMut) -> io::Result<Vec<(&Path, bool)>> {
    let Some(item) = document.get("include") else {
        return Ok(Vec::new());
    };
    if let Some(array) = item.as_array() {
        return array
            .iter()
            .map(|value| match value {
                Value::String(path) => include_path(path.value(), false),
                Value::InlineTable(table) => include_table(table),
                _ => Err(invalid("Cargo 配置 include 必须是路径或表")),
            })
            .collect();
    }
    if let Some(tables) = item.as_array_of_tables() {
        return tables.iter().map(|table| include_table(table)).collect();
    }
    Err(invalid("Cargo 配置 include 必须是数组"))
}

fn include_table(table: &dyn TableLike) -> io::Result<(&Path, bool)> {
    let path = table
        .get("path")
        .and_then(Item::as_str)
        .ok_or_else(|| invalid("Cargo 配置 include 缺少路径"))?;
    let optional = match table.get("optional") {
        Some(item) => item
            .as_bool()
            .ok_or_else(|| invalid("Cargo 配置 include 的 optional 必须是布尔值"))?,
        None => false,
    };
    include_path(path, optional)
}

fn include_path(path: &str, optional: bool) -> io::Result<(&Path, bool)> {
    let file = Path::new(path);
    if file.extension().is_none_or(|extension| extension != "toml")
        || path.contains(['*', '?', '[', ']', '{', '}'])
    {
        return Err(invalid("Cargo 配置 include 必须使用具体的 TOML 文件路径"));
    }
    Ok((file, optional))
}

fn real_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn relative_path(app: &Path, file: &Path) -> PathBuf {
    let shared = app
        .components()
        .zip(file.components())
        .take_while(|(left, right)| left == right)
        .count();
    // Windows 上不同盘符无法表达为同一相对路径，指纹仍只保留其散列。
    if shared == 0 {
        return file.to_path_buf();
    }
    app.components()
        .skip(shared)
        .map(|_| "..".as_ref())
        .chain(
            file.components()
                .skip(shared)
                .map(std::path::Component::as_os_str),
        )
        .collect()
}

fn hash_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u64).to_be_bytes());
    hasher.update(field);
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
