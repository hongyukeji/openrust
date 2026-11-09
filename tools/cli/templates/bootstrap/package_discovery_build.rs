//! 为当前 Cargo 构建选取并校验已安装的包发现清单。

use sha2::{Digest as _, Sha256};
use std::{
    env, fs, io,
    io::Write as _,
    path::{Path, PathBuf},
};

pub const DIRECTORY: &str = "bootstrap/packages";
pub const VERSION: &str = "// openrust-package-plan-v2";
pub const END_INPUTS: &str = "// end-package-plan-inputs";

const EMPTY_SOURCE: &str = r#"
pub(crate) fn providers() -> Vec<::openrust::foundation::PackageProvider> {
    Vec::new()
}
#[allow(unused_imports, reason = "应用按需引用发现的 facade")]
pub(crate) mod facades {}
"#;

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn config_digest(path: &Path) -> io::Result<String> {
    match fs::read(path) {
        Ok(bytes) => Ok(digest(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("-".into()),
        Err(error) => Err(error),
    }
}

pub fn feature_key(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

pub fn plan_path(target: &str, mut features: Vec<String>) -> PathBuf {
    features.sort_unstable();
    features.dedup();
    let key = format!("{target}\n{}", features.join("\n"));
    Path::new(DIRECTORY).join(format!("{}.rs", digest(key.as_bytes())))
}

pub fn compile() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let source = if Path::new(DIRECTORY).try_exists()? {
        writeln!(stdout, "cargo:rerun-if-changed={DIRECTORY}")?;
        let target = env::var("TARGET").map_err(io::Error::other)?;
        // 交叉核对 Cargo 的两组输入，排除单独继承的环境值或 RUSTFLAGS 注入的 cfg。
        let features: Vec<_> = env::var("CARGO_CFG_FEATURE")
            .unwrap_or_default()
            .split(',')
            .filter(|name| !name.is_empty())
            .map(feature_key)
            .filter(|key| env::var_os(format!("CARGO_FEATURE_{key}")).is_some())
            .collect();
        let plan = plan_path(&target, features.clone());
        let source = fs::read_to_string(&plan).map_err(|error| {
            io::Error::other(format!(
                "无法读取 {target}、应用特性 {features:?} 的 Provider 清单（{}）：{error}。请以相同 Cargo 特性和目标运行 openrust package:discover --install。",
                plan.display()
            ))
        })?;
        verify_inputs(&source, &mut stdout)?;
        source
    } else {
        // 观察已存在的上级目录，首次安装时重新选择。
        writeln!(stdout, "cargo:rerun-if-changed=bootstrap")?;
        EMPTY_SOURCE.to_owned()
    };
    let out = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("Cargo 未设置 OUT_DIR"))?,
    );
    let output = out.join("openrust_package_providers.rs");
    if fs::read(&output).as_deref().ok() != Some(source.as_bytes()) {
        fs::write(output, source)?;
    }
    Ok(())
}

fn verify_inputs(source: &str, directives: &mut impl io::Write) -> io::Result<()> {
    let mut lines = source.lines();
    if lines.next() != Some(VERSION) {
        return Err(io::Error::other(
            "Provider 清单格式需要 v2，请运行 openrust package:discover --install 重新生成",
        ));
    }
    let mut application = false;
    for line in lines {
        if line == END_INPUTS && application {
            return Ok(());
        }
        let (expected, path) = line
            .strip_prefix("// ")
            .and_then(|line| line.split_once('\t'))
            .filter(|(hash, path)| {
                (hash.len() == 64 || *hash == "-")
                    && !path.is_empty()
                    && Path::new(path).is_relative()
            })
            .ok_or_else(|| {
                io::Error::other(
                    "Provider 清单输入无效，请运行 openrust package:discover --install",
                )
            })?;
        let path = Path::new(path);
        let watch = if expected == "-" {
            path.parent()
                .filter(|parent| parent.is_dir())
                .unwrap_or(path)
        } else {
            path
        };
        writeln!(directives, "cargo:rerun-if-changed={}", watch.display())?;
        let current = config_digest(path).map_err(|error| {
            io::Error::other(format!("无法读取 Provider 清单输入 {}：{error}。请运行 openrust package:discover --install。", path.display()))
        })?;
        if current != expected {
            return Err(io::Error::other(format!(
                "Provider 清单输入 {} 已变化，请运行 openrust package:discover --install",
                path.display()
            )));
        }
        application |= path == Path::new("Cargo.toml");
    }
    Err(io::Error::other(
        "Provider 清单输入不完整，请运行 openrust package:discover --install",
    ))
}
