//! key:generate 的密钥材料和环境文件更新。宿主负责提供已生效的配置及环境文件路径。

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{Error, Result};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) fn generate_key(cipher: &str) -> Result<String> {
    let size = crate::config::app_cipher_key_size(cipher);
    let mut key = zeroize::Zeroizing::new([0; 32]);
    getrandom::fill(&mut key[..size]).map_err(|_| failure("无法生成应用密钥"))?;
    Ok(format!("base64:{}", STANDARD.encode(&key[..size])))
}

pub(super) struct StagedKeyFile<'a> {
    path: &'a Path,
    temporary: std::path::PathBuf,
    original: zeroize::Zeroizing<String>,
    committed: bool,
}

impl StagedKeyFile<'_> {
    pub(super) fn commit(mut self) -> Result<()> {
        require_regular_file(self.path)?;
        let current = zeroize::Zeroizing::new(
            fs::read_to_string(self.path).map_err(|_| failure("无法重新读取应用环境文件"))?,
        );
        if *current != *self.original {
            return Err(failure("环境文件在写入前已发生变化"));
        }
        fs::rename(&self.temporary, self.path).map_err(|_| failure("无法替换应用环境文件"))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedKeyFile<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(super) fn stage_key_file<'a>(
    path: &'a Path,
    variable: &str,
    expected: &str,
    new_key: &str,
    external_key_present: bool,
) -> Result<StagedKeyFile<'a>> {
    require_regular_file(path)?;
    let original = zeroize::Zeroizing::new(
        fs::read_to_string(path).map_err(|_| failure("无法读取应用环境文件"))?,
    );
    let updated = replace_assignment(&original, variable, expected, new_key, external_key_present)?;
    let parent = path.parent().ok_or_else(|| failure("环境文件缺少父目录"))?;
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".openrust-key-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| failure("无法创建环境文件临时副本"))?;
    let staged = StagedKeyFile {
        path,
        temporary,
        original,
        committed: false,
    };
    let written = file
        .write_all(updated.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|_| failure("无法写入环境文件临时副本"))?;
    Ok(staged)
}

fn parsed_key(text: &str, variable: &str) -> Result<Option<zeroize::Zeroizing<String>>> {
    let mut found = None;
    for entry in dotenvy::from_read_iter(text.as_bytes()) {
        let (name, value) = entry.map_err(|_| failure("应用环境文件格式无效"))?;
        let value = zeroize::Zeroizing::new(value);
        if name == variable {
            if found.is_some() {
                return Err(failure("应用密钥占位必须是唯一的原始赋值"));
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn replace_assignment(
    original: &str,
    variable: &str,
    expected: &str,
    key: &str,
    external_key_present: bool,
) -> Result<zeroize::Zeroizing<String>> {
    let configured = parsed_key(original, variable)?.ok_or_else(|| {
        failure(if external_key_present {
            "应用密钥已由外部环境提供，文件中没有匹配项"
        } else {
            "环境文件缺少应用密钥占位"
        })
    })?;
    if configured.as_str() != expected {
        return Err(failure(if external_key_present {
            "应用密钥已由外部环境提供，文件中的值与生效配置不匹配"
        } else {
            "环境文件中的应用密钥与生效配置不匹配"
        }));
    }

    // dotenvy 返回逻辑赋值而没有字节位置。给候选物理行标序，解析后定位真实赋值，
    // 从而避开其他多行引号内看起来相同的文本。
    let raw_assignment = zeroize::Zeroizing::new(format!("{variable}={expected}"));
    let marker = "openrust-key-probe-";
    let mut probe = zeroize::Zeroizing::new(String::with_capacity(original.len()));
    for (index, line) in original.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\r', '\n']);
        if body == raw_assignment.as_str() {
            write!(probe, "{variable}={marker}{index}{}", &line[body.len()..])
                .map_err(|_| failure("无法识别应用密钥赋值"))?;
        } else {
            probe.push_str(line);
        }
    }
    let line_index = parsed_key(&probe, variable)?
        .as_deref()
        .and_then(|value| value.strip_prefix(marker))
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| failure("应用密钥占位必须是唯一的原始赋值"))?;

    let mut output = zeroize::Zeroizing::new(String::with_capacity(original.len()));
    for (index, line) in original.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\r', '\n']);
        if index == line_index && body == raw_assignment.as_str() {
            write!(output, "{variable}={key}{}", &line[body.len()..])
                .map_err(|_| failure("无法替换应用密钥赋值"))?;
        } else {
            output.push_str(line);
        }
    }
    if parsed_key(&output, variable)?
        .as_deref()
        .map(String::as_str)
        != Some(key)
    {
        return Err(failure("无法确认应用密钥原始赋值已更新"));
    }
    Ok(output)
}

fn require_regular_file(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        Ok(())
    } else {
        Err(failure("应用环境文件必须是已存在的普通文件"))
    }
}

fn failure(message: &'static str) -> Error {
    Error::custom("app_key_generation_failed", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_only_exact_key_assignment_and_preserves_line_endings() -> Result<()> {
        let input = "# APP_KEY=ignored\r\nAPP_NAME=Demo\r\nAPP_KEY=\r\nAPP_PREVIOUS_KEYS=old\r\n";
        let updated = replace_assignment(input, "APP_KEY", "", "base64:synthetic", false)?;
        assert_eq!(
            updated.as_str(),
            input.replacen("APP_KEY=\r\n", "APP_KEY=base64:synthetic\r\n", 1)
        );
        for invalid in [
            "# APP_KEY=\n",
            "APP_KEY=different\n",
            "APP_KEY=\nAPP_KEY=\n",
            "APP_KEY=\nexport APP_KEY=\n",
            "APP_KEY=\nexport\tAPP_KEY=\n",
            "NOTE=\"before\nAPP_KEY=\nafter\"\n",
            "APP_KEY=\"\"\n",
        ] {
            assert!(replace_assignment(invalid, "APP_KEY", "", "base64:synthetic", false).is_err());
        }
        Ok(())
    }

    #[test]
    fn updates_logical_key_without_touching_similar_lines_in_multiline_values() -> Result<()> {
        let input = "NOTE=\"before\nSHOP_KEY=\nafter\"\nSHOP_KEY=\nAPP_KEY=unrelated\n";
        let updated = replace_assignment(input, "SHOP_KEY", "", "base64:synthetic", false)?;
        assert_eq!(
            updated.as_str(),
            "NOTE=\"before\nSHOP_KEY=\nafter\"\nSHOP_KEY=base64:synthetic\nAPP_KEY=unrelated\n"
        );
        Ok(())
    }

    #[test]
    fn staged_key_file_refuses_changed_destination_and_cleans_temporary_file() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join(".env");
        fs::write(&path, "APP_KEY=old\n").map_err(Error::internal)?;
        let staged = stage_key_file(&path, "APP_KEY", "old", "base64:new", false)?;
        let temporary = staged.temporary.clone();
        fs::write(&path, "APP_KEY=changed\n").map_err(Error::internal)?;
        assert!(staged.commit().is_err());
        assert_eq!(
            fs::read_to_string(&path).map_err(Error::internal)?,
            "APP_KEY=changed\n"
        );
        assert!(!temporary.exists());
        Ok(())
    }

    #[test]
    fn generated_key_uses_laravel_prefix_and_cipher_size() -> Result<()> {
        for (cipher, size) in [
            ("AES-128-CBC", 16),
            ("aes-128-gcm", 16),
            ("AES-256-CBC", 32),
            ("aes-256-gcm", 32),
            ("unknown", 32),
        ] {
            let key = generate_key(cipher)?;
            let encoded = key
                .strip_prefix("base64:")
                .ok_or_else(|| failure("缺少 base64 前缀"))?;
            assert_eq!(
                STANDARD.decode(encoded).map_err(Error::internal)?.len(),
                size
            );
        }
        Ok(())
    }

    #[test]
    fn writes_selected_file_and_preserves_it_on_stale_key() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join(".env.testing");
        fs::write(&path, "APP_KEY=old\nAPP_PREVIOUS_KEYS=prior\n").map_err(Error::internal)?;
        assert!(stage_key_file(&path, "APP_KEY", "external", "base64:new", true).is_err());
        assert_eq!(
            fs::read_to_string(&path).map_err(Error::internal)?,
            "APP_KEY=old\nAPP_PREVIOUS_KEYS=prior\n"
        );
        stage_key_file(&path, "APP_KEY", "old", "base64:new", false)?.commit()?;
        assert_eq!(
            fs::read_to_string(&path).map_err(Error::internal)?,
            "APP_KEY=base64:new\nAPP_PREVIOUS_KEYS=prior\n"
        );
        Ok(())
    }
}
