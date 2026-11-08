use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::DatabaseBackend;

/// Cargo 与 `BuildKit` 使用同一份相对目录关系。
pub(crate) struct SourceLayout {
    source: PathBuf,
    context: PathBuf,
    application: String,
    framework: String,
}

impl SourceLayout {
    pub(crate) fn new(source: &Path, project: &Path) -> anyhow::Result<Self> {
        let source = source.canonicalize()?;
        let project = project.canonicalize()?;
        let common = project
            .ancestors()
            .find(|path| path.has_root() && source.starts_with(path))
            .context("框架与应用须位于可用相对路径连接的同一卷")?;
        let application = project.strip_prefix(common)?;
        let framework = source.strip_prefix(common)?;
        let mut context = PathBuf::new();
        for _ in application.components() {
            context.push("..");
        }
        if !framework.as_os_str().is_empty() {
            context.push(framework);
        }
        let application = portable_path(application)?;
        let framework = portable_path(framework)?;
        Ok(Self {
            source,
            context,
            application,
            framework,
        })
    }

    pub(crate) fn dependency_path(&self, path: &str) -> anyhow::Result<String> {
        let dependency = self.source.join(path).canonicalize()?;
        let relative = dependency
            .strip_prefix(&self.source)
            .with_context(|| format!("本地依赖 {path} 超出框架构建上下文"))?;
        portable_path(&self.context.join(relative))
    }

    pub(crate) fn artisan_source(&self) -> anyhow::Result<String> {
        let manifest = portable_path(&self.context.join("tools/cli/Cargo.toml"))?;
        let literal = manifest.replace('\'', "'\\''");
        Ok(include_str!("../../../artisan").replace(
            "cli_manifest='tools/cli/Cargo.toml'",
            &format!("cli_manifest='{literal}'"),
        ))
    }

    pub(crate) fn write_templates(
        &self,
        project: &Path,
        name: &str,
        database: Option<DatabaseBackend>,
    ) -> anyhow::Result<()> {
        fs::write(
            project.join("Dockerfile"),
            include_str!("../templates/deploy/Dockerfile").replace("{{name}}", name),
        )?;
        fs::write(
            project.join(".dockerignore"),
            include_str!("../templates/deploy/.dockerignore"),
        )?;
        fs::write(project.join("docker-compose.yml"), self.compose(database)?)?;
        if database == Some(DatabaseBackend::Postgres) {
            let directory = project.join(".docker");
            fs::create_dir(&directory)?;
            let bootstrap = directory.join("postgres-bootstrap.sh");
            fs::write(
                &bootstrap,
                include_str!("../templates/deploy/postgres-bootstrap.sh"),
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(bootstrap, fs::Permissions::from_mode(0o644))?;
            }
        }
        Ok(())
    }

    fn compose(&self, database: Option<DatabaseBackend>) -> anyhow::Result<String> {
        let (environment, dependency, services) = match database {
            None => ("", "", ""),
            Some(DatabaseBackend::Mysql) => (
                include_str!("../templates/deploy/mysql-env.yml"),
                "      mysql:\n        condition: service_healthy\n",
                include_str!("../templates/deploy/mysql.yml"),
            ),
            Some(DatabaseBackend::Postgres) => (
                include_str!("../templates/deploy/postgres-env.yml"),
                "      postgres:\n        condition: service_healthy\n",
                include_str!("../templates/deploy/postgres.yml"),
            ),
            Some(DatabaseBackend::Sqlite) => (
                "    DB_CONNECTION: sqlite\n    DB_DATABASE: storage/app/database.sqlite\n    DB_TABLE_PREFIX:\n    DB_FOREIGN_KEYS:\n    QUEUE_CONNECTION: ${QUEUE_CONNECTION:-database}\n    QUEUE_REDIS_NAMESPACE:\n",
                "",
                "",
            ),
        };
        let (application_dependency, migration) = if database.is_some() {
            let command = if database == Some(DatabaseBackend::Sqlite) {
                // SQLite Connector 保持拒绝缺失文件，由部署初始化创建新应用文件。
                "    entrypoint: [/bin/sh, -ec]\n    command:\n      - |\n        umask 077\n        mkdir -p storage/app\n        if [ ! -e storage/app/database.sqlite ]; then\n          (set -C; : > storage/app/database.sqlite)\n        fi\n        exec /app/artisan migrate\n"
            } else {
                "    entrypoint: [/app/artisan]\n    command: [migrate]\n"
            };
            (
                "      migrate:\n        condition: service_completed_successfully\n",
                include_str!("../templates/deploy/migrate.yml")
                    .replace("{{command}}\n", command)
                    .replace("{{database_dependency}}\n", dependency),
            )
        } else {
            (
                "      storage-init:\n        condition: service_completed_successfully\n      redis:\n        condition: service_healthy\n",
                String::new(),
            )
        };
        Ok(include_str!("../templates/deploy/docker-compose.yml")
            .replace("{{environment}}\n", environment)
            .replace("{{application_dependency}}\n", application_dependency)
            .replace("{{migration}}\n", &migration)
            .replace("{{database}}\n", services)
            .replace(
                "{{framework_context}}",
                &compose_string(&portable_path(&self.context)?),
            )
            .replace("{{application_layout}}", &compose_string(&self.application))
            .replace("{{framework_layout}}", &compose_string(&self.framework)))
    }
}

fn portable_path(path: &Path) -> anyhow::Result<String> {
    if path.as_os_str().is_empty() {
        return Ok(".".to_owned());
    }
    let parts = path
        .iter()
        .map(|part| part.to_str().context("部署路径必须是有效的 UTF-8"))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(parts.join("/"))
}

fn compose_string(value: &str) -> String {
    serde_json::Value::String(value.replace('$', "$$")).to_string()
}
