#![allow(clippy::expect_used, reason = "测试夹具创建失败时立即终止")]

use super::deployment::SourceLayout;
use super::workspace_entry_tests::create_framework_fixture;
use super::{
    DatabaseBackend, application_directory, create_project, framework_workspace,
    standalone_manifest,
};
use std::fs;
use std::path::Path;

pub(super) fn assert_generated_deployment_artifacts(app: &Path, name: &str) -> anyhow::Result<()> {
    let dockerfile = fs::read_to_string(app.join("Dockerfile"))?;
    assert!(dockerfile.contains(&format!("\"/out/{name}\"")), "{name}");
    assert!(
        dockerfile.contains(&format!("ENTRYPOINT [\"/app/{name}\"]")),
        "{name}"
    );
    assert!(
        fs::read_to_string(app.join("README.md"))?.contains(&format!("./{name}")),
        "{name}"
    );
    Ok(())
}

#[test]
fn standalone_apps_inherit_relative_driver_patches_without_changing_source() {
    let root =
        std::env::temp_dir().join(format!("openrust-cli-driver-patch-{}", std::process::id()));
    fs::create_dir_all(&root).expect("创建独立测试目录");
    create_framework_fixture(&root);
    let driver = root.join("vendor/launchbadge/sqlx-mysql");
    fs::create_dir_all(&driver).expect("创建补丁目录");
    let orm = root.join("vendor/seaql/sea-orm");
    fs::create_dir_all(&orm).expect("创建ORM补丁目录");
    let source_path = root.join("Cargo.toml");
    let mut source = fs::read_to_string(&source_path).expect("读取源清单");
    source.push_str("\n[patch.crates-io]\nsqlx-mysql = { path = \"vendor/launchbadge/sqlx-mysql\" }\nsea-orm = { path = \"vendor/seaql/sea-orm\" }\n");
    fs::write(&source_path, &source).expect("登记驱动补丁");
    let app = root.join("application");
    fs::create_dir_all(&app).expect("创建应用目录");
    fs::write(app.join("Cargo.toml"), "[package]\nname=\"application\"\nversion=\"0.0.0\"\n[dependencies]\nopenrust.workspace=true\n").expect("写应用清单");
    standalone_manifest(&root, &app, false).expect("独立化清单");
    let generated = fs::read_to_string(app.join("Cargo.toml"))
        .expect("读取生成清单")
        .parse::<toml_edit::DocumentMut>()
        .expect("解析生成清单");
    assert_eq!(
        generated["patch"]["crates-io"]["sqlx-mysql"]["path"].as_str(),
        Some("../vendor/launchbadge/sqlx-mysql")
    );
    assert_eq!(
        generated["patch"]["crates-io"]["sea-orm"]["path"].as_str(),
        Some("../vendor/seaql/sea-orm")
    );
    assert_eq!(fs::read_to_string(&source_path).expect("源清单"), source);
    fs::remove_dir_all(root).expect("清理本测试目录");
}

#[test]
fn creates_independent_apps_without_changing_source_workspace() {
    let root =
        std::env::temp_dir().join(format!("openrust-cli-independent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("创建临时目录");
    create_framework_fixture(&root);
    let source = framework_workspace(&root).expect("解析源码 workspace");
    assert!(source.packages.iter().any(|package| {
        package.name == "openrust-framework"
            && package.manifest_path
                == root
                    .join("vendor/openrust/framework/Cargo.toml")
                    .canonicalize()
                    .expect("源码路径")
    }));
    let original = fs::read_to_string(root.join("Cargo.toml")).expect("读取源配置");
    for (name, backend) in [
        ("blog", DatabaseBackend::Postgres),
        ("shop", DatabaseBackend::Mysql),
    ] {
        create_project(name, Some(backend), false, false, false, false, &root)
            .expect("生成独立应用");
        let app = root.join(name);
        let manifest = fs::read_to_string(app.join("Cargo.toml")).expect("读取应用配置");
        assert!(manifest.contains("[workspace]"));
        assert!(manifest.contains("database/migrations"));
        assert!(!manifest.contains("modules/example"));
        assert!(!manifest.contains("source-only"));
        let generated = manifest
            .parse::<toml_edit::DocumentMut>()
            .expect("解析应用清单");
        assert_eq!(generated["package"]["name"].as_str(), Some(name));
        assert_eq!(
            generated["dependencies"]["openrust-modules"]["workspace"].as_bool(),
            Some(true)
        );
        assert_eq!(
            generated["workspace"]["dependencies"]["openrust-modules"]["path"].as_str(),
            Some("../vendor/openrust/modules")
        );
        assert_eq!(
            generated["workspace"]["dependencies"]["openrust"]["package"].as_str(),
            Some("openrust-framework")
        );
        assert_eq!(
            generated["workspace"]["dependencies"]["openrust"]["path"].as_str(),
            Some("../vendor/openrust/framework")
        );
        assert_eq!(
            generated["workspace"]["default-members"]
                .as_array()
                .expect("默认成员")
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .collect::<Vec<_>>(),
            vec!["."]
        );
        let metadata = framework_workspace(&app).expect("独立应用 metadata");
        assert_eq!(
            metadata.root,
            app.canonicalize().expect("独立 workspace 根")
        );
        let (resolved, package) = application_directory(&metadata, &app).expect("解析当前应用");
        assert_eq!(resolved, app.canonicalize().expect("应用路径"));
        assert_eq!(package, name);
        let compose =
            fs::read_to_string(app.join("docker-compose.yml")).expect("读取 Compose 配置");
        assert!(
            compose.contains("      migrate:\n        condition: service_completed_successfully")
        );
        assert!(compose.contains(
            "    entrypoint: [/app/artisan]\n    command: [migrate, --force, --no-interaction]\n"
        ));
        assert!(!compose.contains("env_file:"));
        if backend == DatabaseBackend::Postgres {
            assert!(compose.contains("    user: \"postgres:postgres\""));
            assert!(compose.contains("      POSTGRES_USER: postgres"));
            assert!(app.join(".docker/postgres-bootstrap.sh").is_file());
        } else {
            assert!(compose.contains("    image: mysql:8.4"));
        }
    }
    assert_eq!(
        fs::read_to_string(root.join("Cargo.toml")).expect("源配置"),
        original
    );
    fs::remove_dir_all(root).expect("清理本测试目录");
}

#[test]
fn sqlite_compose_sets_migration_arguments_before_creating_database() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let framework = root.path().join("framework");
    let app = root.path().join("sqlite-app");
    fs::create_dir(&framework)?;
    fs::create_dir(&app)?;
    SourceLayout::new(&framework, &app)?.write_templates(
        &app,
        "sqlite-app",
        Some(DatabaseBackend::Sqlite),
    )?;

    let compose = fs::read_to_string(app.join("docker-compose.yml"))?;
    assert!(compose.contains(concat!(
        "    entrypoint: [/bin/sh, -ec]\n",
        "    command:\n",
        "      - |\n",
        "        set -- migrate --force --no-interaction\n",
        "        umask 077\n",
        "        mkdir -p storage/app\n",
        "        if [ ! -e storage/app/database.sqlite ]; then\n",
        "          (set -C; : > storage/app/database.sqlite)\n",
        "        fi\n",
        "        exec /app/artisan \"$$@\"\n",
    )));
    Ok(())
}

#[test]
fn relocated_project_keeps_cargo_and_compose_source_paths() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "openrust-cli-relocation-{}-{}",
        std::process::id(),
        super::MIGRATION_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let original = root.join("old parent");
    let relocated = root.join("new parent");
    let framework = original.join("framework source");
    fs::create_dir_all(&framework)?;
    create_framework_fixture(&framework);
    let project = original.join("minimal-app");
    create_project(
        project.to_str().expect("应用目录编码"),
        None,
        false,
        false,
        false,
        false,
        &framework,
    )?;

    fs::rename(&original, &relocated)?;
    let app = relocated.join("minimal-app");
    let manifest = fs::read_to_string(app.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
    let dependency = manifest["workspace"]["dependencies"]["openrust"]["path"]
        .as_str()
        .expect("框架相对路径");
    assert_eq!(dependency, "../framework source/vendor/openrust/framework");
    assert_eq!(
        app.join(dependency).canonicalize()?,
        relocated
            .join("framework source/vendor/openrust/framework")
            .canonicalize()?
    );

    let compose = fs::read_to_string(app.join("docker-compose.yml"))?;
    assert!(compose.contains("openrust-source: \"../framework source\""));
    assert!(compose.contains("OPENRUST_APP_LAYOUT: \"minimal-app\""));
    assert!(compose.contains("OPENRUST_FRAMEWORK_LAYOUT: \"framework source\""));
    assert!(compose.contains("source: ${OPENRUST_PUBLIC_DIR:-./public}"));
    assert!(compose.contains("source: ${OPENRUST_DATA_DIR:-./docker-data}/redis"));
    assert!(!compose.contains("env_file:"));
    assert!(!compose.contains("DB_CONNECTION:"));
    assert!(!compose.contains("  migrate:"));
    assert_generated_deployment_artifacts(&app, "minimal-app")?;
    assert!(app.join(".dockerignore").is_file());
    assert!(
        fs::read_to_string(app.join("artisan"))?
            .contains("cli_manifest='../framework source/tools/cli/Cargo.toml'")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}
