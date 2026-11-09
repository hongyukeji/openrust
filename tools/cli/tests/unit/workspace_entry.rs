//! 工作区默认应用的 Cargo 入口与共享清单夹具。

#![allow(clippy::expect_used, reason = "测试夹具失败时立即终止")]

use super::{application_directory, framework_workspace, validate_framework_source};
use std::{fs, path::Path};

pub(super) fn create_framework_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("创建框架目录");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"openrust\"\ndefault-run = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[[bin]]\nname = \"app\"\npath = \"src/main.rs\"\n\n[workspace]\nresolver = \"3\"\n# 保留工作区专有配置与成员说明。\ncustom-marker = \"keep-me\"\nexclude = [\"source-only\"]\nmembers = [\"vendor/openrust/framework\", \"vendor/openrust/modules\", \"tools/cli\", \"tools/macros\", \"tools/testing\", \"tools/performance\"]\ndefault-members = [\".\"]\n\n[workspace.package]\nversion = \"0.0.0\"\nedition = \"2024\"\nlicense = \"MIT OR Apache-2.0\"\n\n[workspace.dependencies]\nsha2 = \"0.10\"\ntoml_edit = \"0.25\"\nopenrust = { package = \"openrust-framework\", path = \"vendor/openrust/framework\" }\nopenrust-modules = { path = \"vendor/openrust/modules\" }\nopenrust-translation-database = { path = \"vendor/openrust/translation-database\" }\nopenrust-passport = { path = \"vendor/openrust/passport\" }\ncli = { path = \"tools/cli\" }\nmacros = { path = \"tools/macros\" }\ntesting = { path = \"tools/testing\" }\n",
    )
    .expect("create framework workspace manifest");
    let framework = root.join("vendor/openrust/framework");
    fs::create_dir_all(framework.join("src")).expect("创建独立框架目录");
    fs::write(framework.join("src/lib.rs"), "").expect("创建独立框架库");
    fs::write(
        framework.join("Cargo.toml"),
        "[package]\nname=\"openrust-framework\"\nversion.workspace=true\nedition.workspace=true\n\n[lib]\nname=\"openrust\"\n",
    )
    .expect("框架清单");
    let modules = root.join("vendor/openrust/modules");
    fs::create_dir_all(modules.join("src")).expect("模块扩展目录");
    fs::write(modules.join("src/lib.rs"), "").expect("模块扩展入口");
    fs::write(
        modules.join("Cargo.toml"),
        "[package]\nname=\"openrust-modules\"\nversion.workspace=true\nedition.workspace=true\n",
    )
    .expect("模块扩展清单");
    let translation = root.join("vendor/openrust/translation-database");
    fs::create_dir_all(translation.join("src")).expect("数据库翻译fixture目录");
    fs::write(translation.join("Cargo.toml"), "[package]\nname=\"openrust-translation-database\"\nversion.workspace=true\nedition.workspace=true\n[features]\ndefault=[]\ndb-mysql=[]\ndb-postgres=[]\ndb-sqlite=[]\n").expect("数据库翻译fixture清单");
    fs::write(translation.join("src/lib.rs"), "").expect("数据库翻译fixture源码");
    let passport = root.join("vendor/openrust/passport");
    fs::create_dir_all(passport.join("src")).expect("创建 Passport 夹具目录");
    fs::write(
        passport.join("Cargo.toml"),
        "[package]\nname=\"openrust-passport\"\nversion.workspace=true\nedition.workspace=true\n[features]\ndefault=[]\ndb-mysql=[]\ndb-postgres=[]\ndb-sqlite=[]\n",
    )
    .expect("Passport 夹具清单");
    fs::write(passport.join("src/lib.rs"), "").expect("Passport 夹具入口");
    fs::write(root.join("src/main.rs"), "fn main() {} ").expect("默认应用入口");
    fs::create_dir_all(root.join("bootstrap")).expect("创建应用装配目录");
    fs::write(root.join("bootstrap/app.rs"), "").expect("应用标识文件");
    for (name, path, target) in [
        ("cli", "tools/cli", "src/main.rs"),
        ("macros", "tools/macros", "src/lib.rs"),
        ("testing", "tools/testing", "src/lib.rs"),
        ("performance", "tools/performance", "src/lib.rs"),
    ] {
        let package = root.join(path);
        fs::create_dir_all(package.join("src")).expect("create package source directory");
        fs::write(package.join(target), "").expect("create package target");
        let mut manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n"
        );
        if name == "cli" {
            fs::write(package.join("src/lib.rs"), "").expect("源码 Artisan 工具库入口");
        }
        if name == "macros" {
            manifest.push_str("\n[lib]\nproc-macro = true\n");
        }
        fs::write(package.join("Cargo.toml"), manifest).expect("create workspace member manifest");
    }
    fs::create_dir_all(root.join("apps")).expect("create apps directory");
}

#[test]
fn workspace_root_commands_select_the_root_application() {
    let root = std::env::temp_dir().join(format!("openrust-cli-default-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("创建临时目录");
    create_framework_fixture(&root);
    let workspace = framework_workspace(&root).expect("源码 workspace metadata");
    validate_framework_source(&workspace).expect("独立框架和根默认应用");
    assert!(!root.join("src/lib.rs").exists());
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .expect("根清单")
        .parse::<toml_edit::DocumentMut>()
        .expect("解析根清单");
    assert_eq!(manifest["package"]["default-run"].as_str(), Some("app"));
    let (directory, package) = application_directory(&workspace, &root).expect("默认应用");
    assert_eq!(directory, root.canonicalize().expect("应用路径"));
    assert_eq!(package, "openrust");
    fs::remove_dir_all(root).expect("清理临时目录");
}
