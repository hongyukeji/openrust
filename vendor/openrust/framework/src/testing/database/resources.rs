//! 网络资源的唯一持责者；本地锁、远端见证与专属会话一起交接。

use super::{
    lease::ServerLease,
    marker::{self, MarkerProof},
    ownership::{LockedOwnership, OwnershipJournal, OwnershipState, ResourceKind},
};
use crate::{Error, Result};
use sea_orm::{ConnectionTrait, DbBackend};
use std::path::{Path, PathBuf};

/// 下一次显式测试入口的资源回收结果；不包含凭据、token或连接URL。
#[derive(Clone, Debug, Default)]
pub struct TestDatabaseRecovery {
    /// 已确认删除或已不存在的自有资源数。
    pub reclaimed: usize,
    /// 仍由活跃本机操作持锁的记录数。
    pub active: usize,
    /// 因见证缺失、错配、占用或服务错误保留的记录数。
    pub retained: usize,
}

pub(super) struct OwnedDatabase {
    pub(super) journal: LockedOwnership,
    lease: Option<ServerLease>,
    create_confirmed: bool,
    pub(super) migration_started: bool,
    pub(super) migration_pool: Option<sea_orm::DatabaseConnection>,
}

#[cfg(test)]
pub(super) fn state_root() -> Result<PathBuf> {
    crate::test_database_environment::state_root()
}

#[cfg(not(test))]
pub(super) fn state_root() -> Result<PathBuf> {
    match std::env::var_os("OPENRUST_TEST_DATABASE_STATE_DIR") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(std::env::current_dir()
            .map_err(Error::internal)?
            .join("storage/framework/testing/databases")),
    }
}

fn failure(code: &'static str) -> Error {
    Error::custom(code, "测试数据库归属无法确认，保留资源与回执")
}

impl OwnedDatabase {
    pub(super) async fn plan(
        admin_url: &str,
        root: &Path,
        name: &str,
        kind: ResourceKind,
    ) -> Result<Self> {
        let lease = ServerLease::acquire(admin_url, name).await?;
        let journal = OwnershipJournal::new(root)?.create(
            lease.backend(),
            &lease.server_identity,
            &lease.admin_database,
            name,
            kind,
        )?;
        Ok(Self {
            journal,
            lease: Some(lease),
            create_confirmed: false,
            migration_started: false,
            migration_pool: None,
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.journal.record().database_name
    }
    pub(super) fn lease(&self) -> Result<&ServerLease> {
        self.lease
            .as_ref()
            .ok_or_else(|| failure("test_database_lease_missing"))
    }

    /// 模板名来自另一个已持责的资源；严格CREATE，绝不接管已存在的同名库。
    pub(super) async fn create(&mut self, template: Option<&str>) -> Result<()> {
        self.lease()?.verify().await?;
        let backend = self.lease()?.connection().get_database_backend();
        let sql = match backend {
            DbBackend::MySql => format!("CREATE DATABASE `{}`", self.name()),
            DbBackend::Postgres => match template {
                Some(template) if valid_name(template) => format!(
                    "CREATE DATABASE \"{}\" TEMPLATE \"{template}\"",
                    self.name()
                ),
                None => format!("CREATE DATABASE \"{}\"", self.name()),
                _ => return Err(failure("test_database_template_invalid")),
            },
            _ => return Err(failure("test_database_lease_backend_invalid")),
        };
        if let Err(error) = self.lease()?.connection().execute_unprepared(&sql).await {
            self.journal.record_error(
                OwnershipState::Ambiguous,
                "test_database_create_result_unconfirmed",
            )?;
            return Err(Error::internal(error));
        }
        self.create_confirmed = true;
        let proof = MarkerProof::from_record(self.journal.record())?;
        self.lease()?.verify().await?;
        let identity = marker::install_marker(self.lease()?.connection(), &proof).await?;
        self.journal.confirm_owned(identity)
    }

    pub(super) async fn check_marker(&self) -> Result<()> {
        self.lease()?.verify().await?;
        let proof = MarkerProof::from_record(self.journal.record())?;
        if !marker::verify_marker(
            self.lease()?.connection(),
            &proof,
            self.journal.record().marker_identity.as_ref(),
        )
        .await?
        {
            return Err(failure("test_database_marker_mismatch"));
        }
        Ok(())
    }

    pub(super) async fn cleanup(&mut self) -> Result<()> {
        self.cleanup_with_pool_close(false).await
    }

    /// 正常业务池成功关闭的专用入口；恢复、模板和初始化异常仍使用严格 cleanup。
    pub(super) async fn cleanup_after_pool_close(&mut self) -> Result<()> {
        let record = self.journal.record();
        let allow_busy_recheck = self.create_confirmed
            && record.resource_kind == ResourceKind::Target
            && record.state == OwnershipState::Owned
            && record.marker_identity.is_some()
            && self.lease()?.backend() == super::ownership::Backend::Mysql;
        self.cleanup_with_pool_close(allow_busy_recheck).await
    }

    async fn cleanup_with_pool_close(&mut self, allow_busy_recheck: bool) -> Result<()> {
        let outcome = self.cleanup_inner(allow_busy_recheck).await;
        if let Err(error) = &outcome {
            let code = match error.code() {
                "test_database_resource_in_use" => "test_database_resource_in_use",
                "test_database_marker_mismatch" => "test_database_marker_mismatch",
                "test_database_lease_session_changed" => "test_database_lease_session_changed",
                "test_database_lease_lost" => "test_database_lease_lost",
                _ => "test_database_cleanup_unconfirmed",
            };
            // 持久化失败也向调用方返回；不能静默把内存状态冒充可恢复回执。
            self.journal.record_error(OwnershipState::Ambiguous, code)?;
        }
        outcome
    }

    async fn cleanup_inner(&mut self, allow_busy_recheck: bool) -> Result<()> {
        if self.journal.record().state == OwnershipState::Done {
            return Ok(());
        }
        self.close_migration_pool().await?;
        self.lease()?.verify().await?;
        if !self.lease()?.database_exists(self.name()).await? {
            return self.journal.persist(OwnershipState::Done);
        }
        let proof = MarkerProof::from_record(self.journal.record())?;
        if self.journal.record().marker_identity.is_none() {
            if let Some(identity) =
                marker::observe_marker(self.lease()?.connection(), &proof).await?
            {
                if self.journal.record().state == OwnershipState::DropPending {
                    self.journal.record_error(
                        OwnershipState::Ambiguous,
                        "test_database_marker_identity_pending",
                    )?;
                }
                self.journal.confirm_owned(identity)?;
            } else if !self.create_confirmed || self.migration_started {
                return Err(failure("test_database_marker_mismatch"));
            }
        }
        if self.journal.record().marker_identity.is_some() {
            self.check_marker().await?;
        }
        if allow_busy_recheck {
            self.lease()?
                .require_idle_after_pool_close(self.name())
                .await?;
        } else {
            self.lease()?.require_idle(self.name()).await?;
        }
        self.journal.persist(OwnershipState::DropPending)?;
        self.lease()?.verify().await?;
        // 再核标记，不能以恢复开始时的旧快照授权最终DROP。
        if self.journal.record().marker_identity.is_some() {
            self.check_marker().await?;
        }
        self.lease()?.require_idle(self.name()).await?;
        let sql = match self.lease()?.connection().get_database_backend() {
            DbBackend::MySql => format!("DROP DATABASE `{}`", self.name()),
            DbBackend::Postgres => format!("DROP DATABASE \"{}\"", self.name()),
            _ => return Err(failure("test_database_lease_backend_invalid")),
        };
        self.lease()?
            .connection()
            .execute_unprepared(&sql)
            .await
            .map_err(Error::internal)?;
        self.journal.persist(OwnershipState::Done)
    }

    pub(super) async fn close_migration_pool(&mut self) -> Result<()> {
        if let Some(pool) = &self.migration_pool {
            tokio::time::timeout(std::time::Duration::from_secs(2), pool.close_by_ref())
                .await
                .map_err(|_| failure("test_database_pool_close_timeout"))?
                .map_err(Error::internal)?;
        }
        drop(self.migration_pool.take());
        Ok(())
    }

    pub(super) async fn close_lease(&mut self) -> Result<()> {
        if let Some(lease) = self.lease.take() {
            lease.close().await?;
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    name.starts_with("openrust_")
        && name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 只检查本机私有回执，不扫描服务器的openrust前缀数据库。
pub(super) async fn recover(admin_url: &str, root: &Path) -> Result<TestDatabaseRecovery> {
    let journal = OwnershipJournal::new(root)?;
    let mut report = TestDatabaseRecovery::default();
    for path in journal.list_paths()? {
        let locked = match journal.open_try_locked(&path) {
            Ok(Some(record)) => record,
            Ok(None) => {
                report.active += 1;
                continue;
            }
            Err(error) => {
                tracing::warn!(code = error.code(), "测试数据库回执不可核验");
                report.retained += 1;
                continue;
            }
        };
        if locked.record().state == OwnershipState::Done {
            continue;
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
        let acquire = tokio::time::timeout_at(
            deadline,
            ServerLease::acquire(admin_url, &locked.record().database_name),
        )
        .await;
        let lease = match acquire {
            Ok(Ok(lease)) => lease,
            Ok(Err(error)) => {
                tracing::warn!(database=%locked.record().database_name,code=error.code(),"保留未能取得会话屏障的测试资源");
                report.retained += 1;
                continue;
            }
            Err(_) => {
                report.retained += 1;
                continue;
            }
        };
        if lease.backend() != locked.record().backend
            || lease.server_identity != locked.record().server_identity
            || lease.admin_database != locked.record().admin_database
        {
            if let Err(error) = lease.close().await {
                tracing::warn!(code = error.code(), "错配管理会话关闭未确认");
            }
            report.retained += 1;
            continue;
        }
        let mut resource = OwnedDatabase {
            journal: locked,
            lease: Some(lease),
            create_confirmed: false,
            migration_started: true,
            migration_pool: None,
        };
        let cleanup = tokio::time::timeout_at(deadline, resource.cleanup()).await;
        let cleanup = if let Ok(result) = cleanup {
            result
        } else {
            resource
                .journal
                .record_error(OwnershipState::Ambiguous, "test_database_recovery_timeout")?;
            Err(failure("test_database_recovery_timeout"))
        };
        match cleanup {
            Ok(()) => report.reclaimed += 1,
            Err(error) => {
                tracing::warn!(database=%resource.name(),code=error.code(),"保留未核验的测试数据库");
                report.retained += 1;
            }
        }
        if let Err(error) = resource.close_lease().await {
            tracing::warn!(code = error.code(), "恢复管理会话关闭未确认");
        }
    }
    Ok(report)
}

#[cfg(test)]
mod environment_tests {
    use super::*;

    #[test]
    fn journal_root_and_url_stay_fixed_during_clear_env_and_after_jail_directory_deletion()
    -> Result<()> {
        let _guard = crate::lock_test_env();
        let expected_root = state_root()?;
        let expected_url = crate::test_database_environment::database_url();
        let mut during = None;
        let mut jail_directory = None;
        figment::Jail::expect_with(
            #[expect(
                clippy::result_large_err,
                reason = "Figment Jail闭包要求原生figment::Error"
            )]
            |jail| {
                jail.clear_env();
                jail_directory = Some(jail.directory().to_path_buf());
                during = Some((
                    state_root(),
                    crate::test_database_environment::database_url(),
                ));
                Ok(())
            },
        );
        let (during_root, during_url) = during
            .ok_or_else(|| Error::custom("test_jail_capture_missing", "缺少Jail期间读取结果"))?;
        let during_root = during_root?;
        let deleted = jail_directory
            .ok_or_else(|| Error::custom("test_jail_directory_missing", "缺少Jail目录"))?;
        assert!(!deleted.exists());
        // 旧动态实现会在clear_env期间落到此已删除目录，即使退出Jail后环境已恢复。
        assert_eq!(during_root, expected_root);
        assert!(!during_root.starts_with(&deleted));
        assert!(during_url.eq(&expected_url));
        assert_eq!(state_root()?, expected_root);
        assert!(crate::test_database_environment::database_url().eq(&expected_url));
        Ok(())
    }

    #[test]
    fn original_empty_and_dot_component_roots_remain_rejected_without_creating_paths() {
        for raw in ["", ".", "./journal", "../journal"] {
            assert!(
                matches!(OwnershipJournal::new(Path::new(raw)), Err(error) if error.code()=="test_database_journal_path_invalid")
            );
        }
    }
}
