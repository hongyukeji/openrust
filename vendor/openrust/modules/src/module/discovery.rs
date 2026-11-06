//! 同次启动的 module.json 元数据扫描；不选择状态、排序优先级或执行模块代码。

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use glob::{MatchOptions, Pattern};
use serde_json::{Map, Value};

use openrust::core::{Error, Result};

/// 一份发现声明；重复路径和名称在此层原样保留。
#[derive(Clone)]
pub(crate) struct DiscoveredModule {
    /// 按 PHP 默认标量转换得到的模块名称，不应用编译身份或查找别名规则。
    pub name: String,
    /// 实际 module.json 所在目录；`module_directory` 仅留在元数据供 files 使用。
    pub root: PathBuf,
    /// 原始对象及字段顺序，缺省字段不在此层补值。
    pub attributes: Map<String, Value>,
    /// 实际匹配的 module.json 路径；不因目录覆盖而改变。
    pub manifest_path: PathBuf,
}

/// 按输入 glob 顺序扫描；每项内部按路径排序，重复项再次贡献声明。
///
/// 输入是已展开并以 `/*` 结尾的模块目录模式，扫描只追加 `/module.json`。
/// 对应默认 PHP glob：不匹配隐含点目录，非法模式/遍历错误不产生结果；
/// 已匹配 JSON 的读取、解析或必要类型约束失败则返回脱敏错误。
pub(crate) fn scan(paths: &[PathBuf]) -> Result<Vec<DiscoveredModule>> {
    let mut modules = Vec::new();
    for path in paths {
        for manifest_path in matching_manifests(path) {
            modules.push(read_manifest(&manifest_path)?);
        }
    }
    Ok(modules)
}

fn matching_manifests(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::new()];
    for component in path
        .components()
        .chain(std::iter::once(Component::Normal(OsStr::new(
            "module.json",
        ))))
    {
        match component {
            Component::Normal(name)
                if name
                    .as_encoded_bytes()
                    .iter()
                    .any(|byte| b"*?[".contains(byte)) =>
            {
                paths = expand_pattern(paths, name);
            }
            component => {
                for path in &mut paths {
                    path.push(component.as_os_str());
                }
                if matches!(component, Component::Normal(_)) {
                    paths.retain(|path| fs::symlink_metadata(path).is_ok());
                }
            }
        }
    }
    paths.sort();
    paths
}

fn expand_pattern(paths: Vec<PathBuf>, name: &OsStr) -> Vec<PathBuf> {
    let Some(pattern) = name.to_str().and_then(|name| Pattern::new(name).ok()) else {
        return Vec::new();
    };
    let mut matched = Vec::new();
    for path in paths {
        let directory = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &path
        };
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let name = entry.file_name();
            if pattern_matches_name(&pattern, &name) {
                matched.push(path.join(name));
            }
        }
        // read_dir 不返回点入口；PHP glob 只有显式以点开头的模式才可匹配它们。
        if pattern.as_str().starts_with('.') {
            for name in [".", ".."] {
                if pattern_matches_name(&pattern, OsStr::new(name)) {
                    matched.push(path.join(name));
                }
            }
        }
    }
    matched
}

fn pattern_matches_name(pattern: &Pattern, name: &OsStr) -> bool {
    // 不让 glob 的内部目录遍历对非 UTF-8 名称 unwrap；显式根仍保留原 OsStr。
    // 通配匹配只接受 UTF-8 组件，无法表示的名称不做有损替换或伪匹配。
    name.to_str().is_some_and(|name| {
        pattern.matches_with(
            name,
            MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            },
        )
    })
}

fn read_manifest(manifest_path: &Path) -> Result<DiscoveredModule> {
    let bytes = fs::read(manifest_path).map_err(|_| {
        invalid(
            "module_manifest_read_failed",
            "无法读取已发现的 module.json",
        )
    })?;
    let value = decode_json(&bytes)?;
    let Value::Object(attributes) = value else {
        return Err(invalid(
            "module_manifest_shape_invalid",
            "module.json 顶层必须为对象",
        ));
    };
    let name = match attributes.get("name") {
        Some(Value::String(name)) => name.clone(),
        Some(value @ (Value::Bool(_) | Value::Number(_))) => {
            super::priority::priority_label(value)?
        }
        _ => {
            return Err(invalid(
                "module_manifest_name_invalid",
                "module.json 的 name 必须为字符串、布尔值或数值",
            ));
        }
    };
    // Repository 使用 manifest 的真实目录；Manifest 的 files 目录覆盖不改变模块资源根。
    let root = manifest_path
        .parent()
        .ok_or_else(|| {
            invalid(
                "module_manifest_path_invalid",
                "module.json 缺少推导模块目录",
            )
        })?
        .to_path_buf();
    Ok(DiscoveredModule {
        name,
        root,
        attributes,
        manifest_path: manifest_path.to_path_buf(),
    })
}

// 固定64位PHP默认解码：整数-0成为Long(0)，超出i64的正整数成为double。
// 这不是任意JSON选项的模拟；Value不能表示PHP的INF，溢出保持脱敏解析错误。
fn decode_json(bytes: &[u8]) -> Result<Value> {
    let normalized = normalize_integer_negative_zero(bytes);
    let mut value: Value = serde_json::from_slice(&normalized).map_err(|_| {
        invalid(
            "module_manifest_invalid",
            "module.json 无法解析为支持的 JSON 值",
        )
    })?;
    normalize_json_numbers(&mut value)?;
    Ok(value)
}

fn normalize_integer_negative_zero(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = bytes.to_vec();
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if matches!(byte, b'-' | b'0'..=b'9')
            && (index == 0
                || bytes
                    .get(index - 1)
                    .is_some_and(|byte| json_space(*byte) || matches!(byte, b'[' | b',' | b':')))
            && let Some(end) = number_end(bytes, index)
        {
            // 等长替换保留后续字节偏移；浮点/指数形式与字符串内容均不改写。
            if bytes.get(index..end) == Some(b"-0") {
                normalized[index] = b' ';
            }
            index = end;
            continue;
        }
        index += 1;
    }
    normalized
}

fn number_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index)? {
        b'0' => index += 1,
        b'1'..=b'9' => {
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return None,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let digits = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == digits {
            return None;
        }
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let digits = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == digits {
            return None;
        }
    }
    if bytes
        .get(index)
        .is_none_or(|byte| json_space(*byte) || matches!(byte, b',' | b']' | b'}'))
    {
        Some(index)
    } else {
        None
    }
}

const fn json_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r' | b'\t')
}

fn normalize_json_numbers(value: &mut Value) -> Result<()> {
    match value {
        Value::Number(number) => {
            if let Some(unsigned) = number
                .as_u64()
                .filter(|value| *value > i64::MAX.unsigned_abs())
            {
                *number = php_unsigned_number(unsigned)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_json_numbers(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_json_numbers(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[allow(
    clippy::cast_precision_loss,
    reason = "固定64位PHP将超出Long范围的正整数解码为double，按IEEE754舍入"
)]
fn php_unsigned_number(value: u64) -> Result<serde_json::Number> {
    serde_json::Number::from_f64(value as f64).ok_or_else(|| {
        invalid(
            "module_manifest_invalid",
            "module.json 数字无法表示为支持的 JSON 值",
        )
    })
}

/// 对每个原始 providers 字段递归展开数组/对象值，再过滤 PHP falsy 叶子值。
/// 不限为字符串列表，也不去重；后续编译绑定校验由实际加载层负责。
pub(crate) fn providers(modules: &[DiscoveredModule]) -> Vec<Value> {
    let mut providers = Vec::new();
    for module in modules {
        if let Some(value) = module.attributes.get("providers") {
            flatten(value, &mut providers);
        }
    }
    providers
}

fn flatten(value: &Value, providers: &mut Vec<Value>) {
    match value {
        Value::Array(values) => {
            for value in values {
                flatten(value, providers);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                flatten(value, providers);
            }
        }
        value if is_truthy(value) => providers.push(value.clone()),
        _ => {}
    }
}

/// 对 JSON 解码值采用 PHP 布尔规则；对象在关联解码后按数组是否为空判断。
pub(crate) fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty() && value != "0",
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn invalid(code: &'static str, message: &'static str) -> Error {
    Error::custom(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_manifest(root: &Path, directory: &str, document: &str) -> Result<PathBuf> {
        let module = root.join(directory);
        fs::create_dir_all(&module).map_err(Error::internal)?;
        let manifest = module.join("module.json");
        fs::write(&manifest, document).map_err(Error::internal)?;
        Ok(manifest)
    }

    fn failure(result: Result<Vec<DiscoveredModule>>) -> Result<Error> {
        match result {
            Err(error) => Ok(error),
            Ok(_) => Err(invalid("test_expected_failure", "预期拒绝无效模块声明")),
        }
    }

    #[cfg(unix)]
    fn non_utf8_fixture_supported(directory: &Path) -> Result<bool> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        #[cfg(target_os = "macos")]
        const MACOS_EILSEQ: i32 = 92;

        let probe = directory.join(OsString::from_vec(b"non_utf8_fixture_\xff".to_vec()));
        match fs::create_dir(&probe) {
            Ok(()) => {
                fs::remove_dir(probe).map_err(Error::internal)?;
                Ok(true)
            }
            #[cfg(target_os = "macos")]
            Err(error)
                if error.raw_os_error() == Some(MACOS_EILSEQ)
                    && error.kind() == std::io::Error::from_raw_os_error(MACOS_EILSEQ).kind() =>
            {
                assert_eq!(error.raw_os_error(), Some(MACOS_EILSEQ));
                assert_eq!(
                    error.kind(),
                    std::io::Error::from_raw_os_error(MACOS_EILSEQ).kind()
                );
                eprintln!(
                    "NON_UTF8_FIXTURE_UNAVAILABLE=EILSEQ：当前文件系统拒绝非 UTF-8 目录名，\
                     本项 scanner 文件系统回归未执行；kind={:?}, errno={:?}",
                    error.kind(),
                    error.raw_os_error()
                );
                Ok(false)
            }
            Err(error) => Err(Error::internal(error)),
        }
    }

    #[test]
    fn scan_keeps_path_order_duplicates_and_default_hidden_directory_rule() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let first = temporary.path().join("first");
        let second = temporary.path().join("second");
        write_manifest(&first, "Zeta", r#"{"name":"Zeta"}"#)?;
        write_manifest(&first, "Alpha", r#"{"name":"Alpha"}"#)?;
        write_manifest(&first, ".Hidden", r#"{"name":"Hidden"}"#)?;
        write_manifest(&second, "Beta", r#"{"name":"Beta"}"#)?;
        let modules = scan(&[second.join("*"), first.join("*"), first.join("*")])?;
        assert_eq!(
            modules
                .iter()
                .map(|module| module.name.as_str())
                .collect::<Vec<_>>(),
            ["Beta", "Alpha", "Zeta", "Alpha", "Zeta"]
        );
        assert_eq!(modules[1].manifest_path, modules[3].manifest_path);
        assert_eq!(modules[1].root, first.join("Alpha"));
        assert_eq!(modules[1].manifest_path, first.join("Alpha/module.json"));
        Ok(())
    }

    #[test]
    fn missing_root_and_invalid_glob_contribute_nothing() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        assert!(scan(&[])?.is_empty());
        assert!(scan(&[temporary.path().join("missing/*")])?.is_empty());
        assert!(scan(&[temporary.path().join("[/*")])?.is_empty());
        Ok(())
    }

    #[test]
    fn scan_parses_disabled_manifests_and_reports_found_unreadable_json() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        fs::write(
            temporary.path().join("statuses.json"),
            r#"{"Disabled":false}"#,
        )
        .map_err(Error::internal)?;
        write_manifest(temporary.path(), "Disabled", "{private-json-marker")?;
        // 状态文件不是扫描输入；禁止在 JSON 解析前跳过禁用模块。
        let error = failure(scan(&[temporary.path().join("*")]))?;
        assert_eq!(error.code(), "module_manifest_invalid");
        assert!(!error.to_string().contains("private-json-marker"));
        fs::remove_file(temporary.path().join("Disabled/module.json")).map_err(Error::internal)?;
        fs::create_dir(temporary.path().join("Disabled/module.json")).map_err(Error::internal)?;
        assert_eq!(
            failure(scan(&[temporary.path().join("*")]))?.code(),
            "module_manifest_read_failed"
        );
        Ok(())
    }

    #[test]
    fn rejects_non_objects_and_invalid_name_types_without_disclosing_values() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        for (document, code) in [
            (json!(null), "module_manifest_shape_invalid"),
            (json!(false), "module_manifest_shape_invalid"),
            (json!(42), "module_manifest_shape_invalid"),
            (
                json!("private-json-marker"),
                "module_manifest_shape_invalid",
            ),
            (json!([]), "module_manifest_shape_invalid"),
            (json!({}), "module_manifest_name_invalid"),
            (json!({"name":null}), "module_manifest_name_invalid"),
            (json!({"name":[]}), "module_manifest_name_invalid"),
            (json!({"name":{}}), "module_manifest_name_invalid"),
        ] {
            write_manifest(temporary.path(), "Module", &document.to_string())?;
            let error = failure(scan(&[temporary.path().join("*")]))?;
            assert_eq!(error.code(), code);
            assert!(!error.to_string().contains("private-json-marker"));
        }
        // 上游不约束字符串内容；空串、NUL、空白和 Unicode 均原样保留。
        for name in ["", "\0", "private-json-marker\0", " ", "博客-应用", "0"] {
            write_manifest(
                temporary.path(),
                "Module",
                &json!({"name":name}).to_string(),
            )?;
            let modules = scan(&[temporary.path().join("*")])?;
            assert_eq!(modules[0].name, name);
            assert_eq!(modules[0].attributes["name"], json!(name));
        }
        Ok(())
    }

    #[test]
    fn scalar_names_follow_php_conversion_and_keep_original_json_types() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        for (token, expected, raw) in [
            ("false", "", json!(false)),
            ("true", "1", json!(true)),
            ("0", "0", json!(0)),
            ("-0", "0", json!(0)),
            ("2.0", "2", json!(2.0)),
            ("-0.0", "-0", json!(-0.0)),
            ("-0e0", "-0", json!(-0.0)),
            ("0.25", "0.25", json!(0.25)),
            (
                "9223372036854775807",
                "9223372036854775807",
                json!(i64::MAX),
            ),
            (
                "9223372036854775808",
                "9.2233720368548E+18",
                json!(9_223_372_036_854_775_808.0),
            ),
            (
                "18446744073709551615",
                "1.844674407371E+19",
                json!(18_446_744_073_709_551_616.0),
            ),
        ] {
            write_manifest(
                temporary.path(),
                "Module",
                &format!(r#"{{"name":{token}}}"#),
            )?;
            let modules = scan(&[temporary.path().join("*")])?;
            assert_eq!(modules[0].name, expected, "name token: {token}");
            assert_eq!(modules[0].attributes["name"], raw, "name token: {token}");
        }
        Ok(())
    }

    #[test]
    fn directory_override_stays_raw_without_changing_repository_root() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        for directory in [
            json!("relative/elsewhere"),
            json!(""),
            json!("/not-required-to-exist"),
            Value::Null,
            json!(false),
            json!(true),
            json!(0),
            json!([]),
            json!([null, false, {"nested":[0]}]),
            json!({}),
            json!({"nested":["raw"]}),
        ] {
            let document = json!({
                "name":"Module", "module_directory":directory, "priority":["raw"], "custom":true,
            });
            let manifest = write_manifest(temporary.path(), "Module", &document.to_string())?;
            let modules = scan(&[temporary.path().join("*")])?;
            assert_eq!(modules[0].root, temporary.path().join("Module"));
            assert_eq!(modules[0].manifest_path, manifest);
            assert_eq!(modules[0].attributes["module_directory"], directory);
            assert_eq!(modules[0].attributes["priority"], json!(["raw"]));
            assert_eq!(Value::Object(modules[0].attributes.clone()), document);
            assert_eq!(
                modules[0]
                    .attributes
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["name", "module_directory", "priority", "custom"]
            );
        }
        Ok(())
    }

    #[test]
    fn providers_flatten_object_values_and_keep_truthy_scalar_duplicates_in_order() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        write_manifest(
            temporary.path(),
            "Alpha",
            r#"{"name":"Alpha","providers":{"z":"A","a":["A",{"last":[null,false,0,-0.0,"","0",[],{},"B",1,2.5,true,"0.0"," "]}]}}"#,
        )?;
        write_manifest(
            temporary.path(),
            "Beta",
            r#"{"name":"Beta","providers":"A"}"#,
        )?;
        write_manifest(temporary.path(), "Gamma", r#"{"name":"Gamma"}"#)?;
        let modules = scan(&[temporary.path().join("*"), temporary.path().join("*")])?;
        let expected = vec![
            json!("A"),
            json!("A"),
            json!("B"),
            json!(1),
            json!(2.5),
            json!(true),
            json!("0.0"),
            json!(" "),
            json!("A"),
        ];
        let mut repeated = expected.clone();
        repeated.extend(expected);
        assert_eq!(providers(&modules), repeated);
        Ok(())
    }

    #[test]
    fn truthiness_matches_php_json_associative_decode_boundaries() {
        for (value, expected) in [
            (json!(null), false),
            (json!(false), false),
            (json!(true), true),
            (json!(0), false),
            (json!(0.0), false),
            (json!(-0.0), false),
            (json!(-1), true),
            (json!(1), true),
            (json!(u64::MAX), true),
            (json!(""), false),
            (json!("0"), false),
            (json!("00"), true),
            (json!("0.0"), true),
            (json!("-0"), true),
            (json!(" "), true),
            (json!([]), false),
            (json!([false]), true),
            (json!({}), false),
            (json!({"value":null}), true),
        ] {
            assert_eq!(is_truthy(&value), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_name_is_skipped_by_actual_pattern_matcher_without_filesystem() -> Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let pattern = Pattern::new("*").map_err(Error::internal)?;
        let name = OsString::from_vec(vec![b'm', 0xff]);
        assert!(!pattern_matches_name(&pattern, &name));
        assert!(pattern_matches_name(&pattern, OsStr::new("Visible")));
        assert!(!pattern_matches_name(&pattern, OsStr::new(".Hidden")));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn explicit_non_utf8_root_keeps_literal_bytes_and_finds_utf8_module_when_supported()
    -> Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        // 仅在文件系统能够构造原始字节 fixture 时执行；不将能力不足计作 scanner 验证。
        if !non_utf8_fixture_supported(temporary.path())? {
            return Ok(());
        }
        let root = temporary.path().join(OsString::from_vec(vec![b'r', 0xff]));
        let manifest = write_manifest(&root, "Visible", r#"{"name":"Visible"}"#)?;
        let modules = scan(&[root.join("*")])?;
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "Visible");
        assert_eq!(modules[0].root, root.join("Visible"));
        assert_eq!(modules[0].manifest_path, manifest);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn wildcard_skips_non_utf8_siblings_without_panicking_when_supported() -> Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        // 能力预检成功后，所有 fixture/scan 错误与原断言仍必须正常通过。
        if !non_utf8_fixture_supported(temporary.path())? {
            return Ok(());
        }
        write_manifest(temporary.path(), "Visible", r#"{"name":"Visible"}"#)?;
        let hidden_bytes = temporary.path().join(OsString::from_vec(vec![b'm', 0xff]));
        fs::create_dir(&hidden_bytes).map_err(Error::internal)?;
        fs::write(hidden_bytes.join("module.json"), r#"{"name":"Skipped"}"#)
            .map_err(Error::internal)?;
        fs::write(
            temporary.path().join(OsString::from_vec(vec![b'f', 0xfe])),
            "unrelated",
        )
        .map_err(Error::internal)?;
        let modules = scan(&[temporary.path().join("*")])?;
        assert_eq!(
            modules
                .iter()
                .map(|module| module.name.as_str())
                .collect::<Vec<_>>(),
            ["Visible"]
        );
        Ok(())
    }

    #[test]
    fn integer_negative_zero_lexing_is_token_exact_and_string_aware() {
        let input =
            br#"{"name":"-0","escaped":"quote\" -0 \\ end","values":[-0,-0.0,-0e0,-0E+0,0]}"#;
        let expected =
            br#"{"name":"-0","escaped":"quote\" -0 \\ end","values":[ 0,-0.0,-0e0,-0E+0,0]}"#;
        assert_eq!(normalize_integer_negative_zero(input), expected);
        assert_eq!(normalize_integer_negative_zero(b"-0"), b" 0");
        for invalid in [
            b"[-00]".as_slice(),
            b"[-0.]",
            b"[-0e]",
            b"[true-0]",
            b"[-0x]",
        ] {
            assert_eq!(normalize_integer_negative_zero(invalid), invalid);
        }
    }

    #[test]
    fn php_64_bit_numbers_normalize_recursively_without_changing_other_values() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let path = write_manifest(
            temporary.path(),
            "Numeric",
            r#"{
            "name":"-0", "text":"-0", "flag":false, "nothing":null,
            "numbers":{"array":[-0,-0.0,-0e0,9223372036854775807,9223372036854775808,
                18446744073709551615,-9223372036854775808,-9223372036854775809,-18446744073709551615]},
            "providers":["-0",true,false,null,"0",18446744073709551615]
        }"#,
        )?;
        let module = read_manifest(&path)?;
        assert_eq!(module.name, "-0");
        assert_eq!(module.attributes["text"], "-0");
        assert_eq!(module.attributes["flag"], false);
        assert!(module.attributes["nothing"].is_null());
        let values = &module.attributes["numbers"]["array"];
        assert_eq!(values[0].as_i64(), Some(0));
        assert!(!values[0].is_f64());
        for index in [1, 2] {
            assert!(values[index].is_f64());
            assert!(
                values[index]
                    .as_f64()
                    .is_some_and(|number| number == 0.0 && number.is_sign_negative())
            );
        }
        assert_eq!(values[3].as_i64(), Some(i64::MAX));
        assert!(!values[3].is_f64());
        for (index, expected) in [
            (4, 9_223_372_036_854_775_808.0),
            (5, 18_446_744_073_709_551_616.0),
            (7, -9_223_372_036_854_775_808.0),
            (8, -18_446_744_073_709_551_616.0),
        ] {
            assert!(values[index].is_f64());
            assert_eq!(values[index].as_f64(), Some(expected));
        }
        assert_eq!(values[6].as_i64(), Some(i64::MIN));
        assert!(!values[6].is_f64());
        assert_eq!(module.attributes["providers"][0], "-0");
        assert_eq!(module.attributes["providers"][1], true);
        assert_eq!(module.attributes["providers"][2], false);
        assert!(module.attributes["providers"][3].is_null());
        assert_eq!(module.attributes["providers"][4], "0");
        assert_eq!(
            providers(&[module]),
            vec![
                json!("-0"),
                json!(true),
                json!(18_446_744_073_709_551_616.0)
            ]
        );
        Ok(())
    }

    #[test]
    fn non_finite_php_double_has_an_explicit_value_representation_boundary() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        write_manifest(
            temporary.path(),
            "Infinite",
            r#"{"name":"private-json-marker","priority":1e999}"#,
        )?;
        let error = failure(scan(&[temporary.path().join("*")]))?;
        assert_eq!(error.code(), "module_manifest_invalid");
        assert!(!error.to_string().contains("private-json-marker"));
        assert!(!error.to_string().contains("1e999"));
        Ok(())
    }
}
