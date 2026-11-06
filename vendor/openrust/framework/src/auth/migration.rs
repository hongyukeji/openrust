//! 默认认证器使用的数据库 schema。

use crate::database::{
    DatabaseTableNames, dialect,
    sea_orm::{DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

fn migration_table(logical: &str, backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "OpenRust migration requires MigrationServiceProvider or TestDatabase table-name scope"
                .into(),
        )
    })?;
    names
        .quote(logical, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(logical)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

#[derive(DeriveIden)]
enum TbUsers {
    Id,
    Email,
    PasswordHash,
    RememberToken,
    EmailVerifiedAt,
    DisabledAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TbPersonalAccessTokens {
    Id,
    UserId,
    Name,
    TokenPrefix,
    TokenHash,
    Abilities,
    ExpiresAt,
    RevokedAt,
    LastUsedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthRefreshTokens {
    Id,
    UserId,
    FamilyId,
    TokenHash,
    ExpiresAt,
    ConsumedAt,
    RevokedAt,
    ReplacedByHash,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbVerificationCodes {
    KeyHash,
    ChallengeId,
    CodeHash,
    CodeDigits,
    AttemptCount,
    MaxAttempts,
    IssuedAt,
    ExpiresAt,
    ResendAfterAt,
    ConsumedAt,
}

#[derive(DeriveIden)]
enum TbExternalIdentities {
    Id,
    UserId,
    IdentityHash,
    Provider,
    Issuer,
    Subject,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbExternalIdentityRequests {
    StateHash,
    Provider,
    BrowserBindingHash,
    EncryptedPayload,
    ExpiresAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthExternalSessions {
    IdHash,
    UserId,
    Provider,
    Issuer,
    SubjectHash,
    SessionHash,
    RefreshFamilyId,
    EncryptedSessionId,
    SessionDeletedAt,
    ChallengeHash,
    ExpiresAt,
    RevokedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthExternalLogoutTokens {
    Provider,
    Issuer,
    TokenIdHash,
    ExpiresAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthExternalLogoutRequests {
    StateHash,
    Provider,
    AssociationIdHash,
    PostLogoutRedirectUri,
    ExpiresAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthExternalLogoutScopes {
    Provider,
    Issuer,
    ScopeHash,
    ExpiresAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TbAuthRoles {
    Id,
    Name,
}

#[derive(DeriveIden)]
enum TbAuthPermissions {
    Id,
    Name,
}

#[derive(DeriveIden)]
enum TbAuthUserRoles {
    Id,
    UserId,
    RoleId,
}

#[derive(DeriveIden)]
enum TbAuthRolePermissions {
    Id,
    RoleId,
    PermissionId,
}

/// 创建由框架管理的账户、通用 Session 和个人令牌表。
///
/// 应用应将此 migration 加入现有 SeaORM migrator。业务用户列仍由应用管理，并通过稳定的 `tb_users.id` 关联。
/// 已包含SessionMigration的表；自定义Migrator不得再同时登记独立SessionMigration。
pub struct AuthMigration;

/// 创建 refresh token 家族的追加式 schema migration。
pub struct AuthRefreshTokenMigration;

/// 创建共享的一次性验证码表。
pub struct AuthVerificationCodeMigration;

/// 创建 [`crate::auth::Policy`] 使用的追加式角色与权限 schema。
pub struct AuthPolicyMigration;

/// 创建外部身份绑定与一次性 OIDC 登录事务表。
pub struct AuthExternalIdentityMigration;

/// 创建可跨应用副本查询的外部登录关联与后通道退出重放表。
pub struct AuthExternalSessionMigration;

impl MigrationName for AuthExternalSessionMigration {
    fn name(&self) -> &'static str {
        "m20261027_000007_openrust_external_sessions"
    }
}

#[async_trait::async_trait]
#[allow(
    clippy::too_many_lines,
    reason = "SSO 关联、重放与回调 state schema 同属一个追加迁移"
)]
impl MigrationTrait for AuthExternalSessionMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let sessions = migration_table("auth_external_sessions", backend)?;
        let logout_tokens = migration_table("auth_external_logout_tokens", backend)?;
        let logout_requests = migration_table("auth_external_logout_requests", backend)?;
        let logout_scopes = migration_table("auth_external_logout_scopes", backend)?;
        let users = migration_table("users", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(sessions.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::IdHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::Issuer)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::SubjectHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::SessionHash)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::RefreshFamilyId)
                            .string_len(32)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::EncryptedSessionId)
                            .text()
                            .null(),
                    )
                    .col(
                        dialect::timestamp_utc(TbAuthExternalSessions::SessionDeletedAt, backend)?
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalSessions::ChallengeHash)
                            .string_len(64)
                            .null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthExternalSessions::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(TbAuthExternalSessions::RevokedAt, backend)?.null())
                    .col(dialect::timestamp_utc(
                        TbAuthExternalSessions::CreatedAt,
                        backend,
                    )?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_auth_external_session_user")
                            .from(sessions.clone(), TbAuthExternalSessions::UserId)
                            .to(users, TbUsers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(logout_requests.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutRequests::StateHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutRequests::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutRequests::AssociationIdHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutRequests::PostLogoutRedirectUri)
                            .string_len(2048)
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthExternalLogoutRequests::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(
                        TbAuthExternalLogoutRequests::CreatedAt,
                        backend,
                    )?)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(logout_scopes.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutScopes::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutScopes::Issuer)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutScopes::ScopeHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        dialect::timestamp_utc(TbAuthExternalLogoutScopes::ExpiresAt, backend)?
                            .null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthExternalLogoutScopes::CreatedAt,
                        backend,
                    )?)
                    .primary_key(
                        Index::create()
                            .col(TbAuthExternalLogoutScopes::Provider)
                            .col(TbAuthExternalLogoutScopes::Issuer)
                            .col(TbAuthExternalLogoutScopes::ScopeHash),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(logout_scopes.clone())
                    .name("ix_tb_auth_logout_scope_expiry")
                    .col(TbAuthExternalLogoutScopes::ExpiresAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(sessions.clone())
                    .name("ix_tb_auth_external_session_sid")
                    .col(TbAuthExternalSessions::Provider)
                    .col(TbAuthExternalSessions::Issuer)
                    .col(TbAuthExternalSessions::SessionHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(sessions.clone())
                    .name("ix_tb_auth_external_session_sub")
                    .col(TbAuthExternalSessions::Provider)
                    .col(TbAuthExternalSessions::Issuer)
                    .col(TbAuthExternalSessions::SubjectHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(sessions.clone())
                    .name("uq_tb_auth_external_challenge")
                    .col(TbAuthExternalSessions::ChallengeHash)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(logout_tokens.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutTokens::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutTokens::Issuer)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthExternalLogoutTokens::TokenIdHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthExternalLogoutTokens::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(
                        TbAuthExternalLogoutTokens::CreatedAt,
                        backend,
                    )?)
                    .primary_key(
                        Index::create()
                            .col(TbAuthExternalLogoutTokens::Provider)
                            .col(TbAuthExternalLogoutTokens::Issuer)
                            .col(TbAuthExternalLogoutTokens::TokenIdHash),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for table in [
            "auth_external_logout_requests",
            "auth_external_logout_scopes",
            "auth_external_logout_tokens",
            "auth_external_sessions",
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(migration_table(table, backend)?)
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

impl MigrationName for AuthExternalIdentityMigration {
    fn name(&self) -> &'static str {
        "m20261027_000006_openrust_external_identities"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AuthExternalIdentityMigration {
    #[allow(
        clippy::too_many_lines,
        reason = "身份表和一次性事务表及其约束需在同一 migration 原子创建"
    )]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let identities = migration_table("external_identities", backend)?;
        let requests = migration_table("external_identity_requests", backend)?;
        let users = migration_table("users", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(identities.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbExternalIdentities::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentities::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentities::IdentityHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentities::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentities::Issuer)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentities::Subject)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbExternalIdentities::CreatedAt,
                        backend,
                    )?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_external_identity_user")
                            .from(identities.clone(), TbExternalIdentities::UserId)
                            .to(users, TbUsers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(identities.clone())
                    .name("uq_tb_external_identity_subject")
                    .unique()
                    .col(TbExternalIdentities::IdentityHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(identities)
                    .name("ix_tb_external_identity_user")
                    .col(TbExternalIdentities::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(requests.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbExternalIdentityRequests::StateHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentityRequests::Provider)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentityRequests::BrowserBindingHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbExternalIdentityRequests::EncryptedPayload)
                            .text()
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbExternalIdentityRequests::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(
                        TbExternalIdentityRequests::CreatedAt,
                        backend,
                    )?)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(requests)
                    .name("ix_tb_external_identity_request_expiry")
                    .col(TbExternalIdentityRequests::ExpiresAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for name in ["external_identity_requests", "external_identities"] {
            manager
                .drop_table(
                    Table::drop()
                        .table(migration_table(name, backend)?)
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

impl MigrationName for AuthRefreshTokenMigration {
    fn name(&self) -> &'static str {
        "m20261025_000002_openrust_auth_refresh_tokens"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AuthRefreshTokenMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let refresh_table = migration_table("auth_refresh_tokens", backend)?;
        let users_table = migration_table("users", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(refresh_table.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthRefreshTokens::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthRefreshTokens::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthRefreshTokens::FamilyId)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthRefreshTokens::TokenHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthRefreshTokens::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(TbAuthRefreshTokens::ConsumedAt, backend)?.null())
                    .col(dialect::timestamp_utc(TbAuthRefreshTokens::RevokedAt, backend)?.null())
                    .col(
                        ColumnDef::new(TbAuthRefreshTokens::ReplacedByHash)
                            .string_len(64)
                            .null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbAuthRefreshTokens::CreatedAt,
                        backend,
                    )?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_auth_refresh_user")
                            .from(refresh_table.clone(), TbAuthRefreshTokens::UserId)
                            .to(users_table, TbUsers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(refresh_table.clone())
                    .name("uq_tb_auth_refresh_hash")
                    .unique()
                    .col(TbAuthRefreshTokens::TokenHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(refresh_table.clone())
                    .name("ix_tb_auth_refresh_family")
                    .col(TbAuthRefreshTokens::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(refresh_table.clone())
                    .name("ix_tb_auth_refresh_user")
                    .col(TbAuthRefreshTokens::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let refresh_table = migration_table("auth_refresh_tokens", manager.get_database_backend())?;
        manager
            .drop_table(Table::drop().table(refresh_table).if_exists().to_owned())
            .await
    }
}

impl MigrationName for AuthMigration {
    fn name(&self) -> &'static str {
        "m20261025_000001_openrust_auth"
    }
}

#[async_trait::async_trait]
// 三张关联表及其跨数据库索引共同构成一个原子迁移 schema。
#[allow(clippy::too_many_lines)]
impl MigrationTrait for AuthMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let users_table = migration_table("users", backend)?;
        let pat_table = migration_table("personal_access_tokens", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(users_table.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbUsers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TbUsers::Email).string_len(320).not_null())
                    .col(ColumnDef::new(TbUsers::PasswordHash).text().not_null())
                    .col(dialect::timestamp_utc(TbUsers::EmailVerifiedAt, backend)?.null())
                    .col(dialect::timestamp_utc(TbUsers::DisabledAt, backend)?.null())
                    .col(dialect::timestamp_utc(TbUsers::CreatedAt, backend)?)
                    .col(dialect::timestamp_utc(TbUsers::UpdatedAt, backend)?)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(users_table.clone())
                    .name("uq_tb_users_email")
                    .unique()
                    .col(TbUsers::Email)
                    .to_owned(),
            )
            .await?;
        crate::session::SessionMigration.up(manager).await?;
        manager
            .create_table(
                Table::create()
                    .table(pat_table.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbPersonalAccessTokens::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbPersonalAccessTokens::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbPersonalAccessTokens::Name)
                            .string_len(120)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbPersonalAccessTokens::TokenPrefix)
                            .string_len(24)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbPersonalAccessTokens::TokenHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(dialect::json(TbPersonalAccessTokens::Abilities, backend)?)
                    .col(dialect::timestamp_utc(TbPersonalAccessTokens::ExpiresAt, backend)?.null())
                    .col(dialect::timestamp_utc(TbPersonalAccessTokens::RevokedAt, backend)?.null())
                    .col(
                        dialect::timestamp_utc(TbPersonalAccessTokens::LastUsedAt, backend)?.null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbPersonalAccessTokens::CreatedAt,
                        backend,
                    )?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_pat_user_id")
                            .from(pat_table.clone(), TbPersonalAccessTokens::UserId)
                            .to(users_table.clone(), TbUsers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(pat_table.clone())
                    .name("uq_tb_pat_token_hash")
                    .unique()
                    .col(TbPersonalAccessTokens::TokenHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(pat_table.clone())
                    .name("ix_tb_pat_user_id")
                    .col(TbPersonalAccessTokens::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let pat_table = migration_table("personal_access_tokens", backend)?;
        let users_table = migration_table("users", backend)?;
        manager
            .drop_table(Table::drop().table(pat_table).if_exists().to_owned())
            .await?;
        crate::session::SessionMigration.down(manager).await?;
        manager
            .drop_table(Table::drop().table(users_table).if_exists().to_owned())
            .await
    }
}

impl MigrationName for AuthVerificationCodeMigration {
    fn name(&self) -> &'static str {
        "m20261026_000004_openrust_verification_codes"
    }
}

impl MigrationName for AuthPolicyMigration {
    fn name(&self) -> &'static str {
        "m20261026_000005_openrust_auth_policy"
    }
}

#[async_trait::async_trait]
// 相关 RBAC 表与索引构成一个追加式迁移单元。
#[allow(clippy::too_many_lines)]
impl MigrationTrait for AuthPolicyMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let users = migration_table("users", backend)?;
        let roles = migration_table("roles", backend)?;
        let permissions = migration_table("permissions", backend)?;
        let user_roles = migration_table("user_roles", backend)?;
        let role_permissions = migration_table("role_permissions", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(roles.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthRoles::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TbAuthRoles::Name).string_len(160).not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(roles.clone())
                    .name("uq_tb_auth_roles_name")
                    .unique()
                    .col(TbAuthRoles::Name)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(permissions.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthPermissions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthPermissions::Name)
                            .string_len(160)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(permissions.clone())
                    .name("uq_tb_auth_permissions_name")
                    .unique()
                    .col(TbAuthPermissions::Name)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(user_roles.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthUserRoles::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthUserRoles::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthUserRoles::RoleId)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_user_roles_user")
                            .from(user_roles.clone(), TbAuthUserRoles::UserId)
                            .to(users, TbUsers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_user_roles_role")
                            .from(user_roles.clone(), TbAuthUserRoles::RoleId)
                            .to(roles.clone(), TbAuthRoles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(user_roles.clone())
                    .name("uq_tb_auth_user_roles_pair")
                    .unique()
                    .col(TbAuthUserRoles::UserId)
                    .col(TbAuthUserRoles::RoleId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(role_permissions.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbAuthRolePermissions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbAuthRolePermissions::RoleId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbAuthRolePermissions::PermissionId)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_role_permissions_role")
                            .from(role_permissions.clone(), TbAuthRolePermissions::RoleId)
                            .to(roles, TbAuthRoles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_auth_role_permissions_permission")
                            .from(
                                role_permissions.clone(),
                                TbAuthRolePermissions::PermissionId,
                            )
                            .to(permissions, TbAuthPermissions::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(role_permissions.clone())
                    .name("uq_tb_auth_role_permissions_pair")
                    .unique()
                    .col(TbAuthRolePermissions::RoleId)
                    .col(TbAuthRolePermissions::PermissionId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for logical in ["role_permissions", "user_roles", "permissions", "roles"] {
            let table = migration_table(logical, backend)?;
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AuthVerificationCodeMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let table = migration_table("verification_codes", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(table.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbVerificationCodes::KeyHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TbVerificationCodes::ChallengeId)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbVerificationCodes::CodeHash)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbVerificationCodes::CodeDigits)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbVerificationCodes::AttemptCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TbVerificationCodes::MaxAttempts)
                            .integer()
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        TbVerificationCodes::IssuedAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(
                        TbVerificationCodes::ExpiresAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(
                        TbVerificationCodes::ResendAfterAt,
                        backend,
                    )?)
                    .col(dialect::timestamp_utc(TbVerificationCodes::ConsumedAt, backend)?.null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(table)
                    .name("ix_tb_verification_expiry")
                    .col(TbVerificationCodes::ExpiresAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = migration_table("verification_codes", manager.get_database_backend())?;
        manager
            .drop_table(Table::drop().table(table).if_exists().to_owned())
            .await
    }
}

/// 为既有账户表追加可撤销的记住我token，不删除或重建账户数据。
pub struct AuthRememberTokenMigration;
impl MigrationName for AuthRememberTokenMigration {
    fn name(&self) -> &'static str {
        "m20261104_000008_openrust_auth_remember_token"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for AuthRememberTokenMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = migration_table("users", manager.get_database_backend())?;
        manager
            .alter_table(
                Table::alter()
                    .table(table)
                    .add_column(
                        ColumnDef::new(TbUsers::RememberToken)
                            .string_len(100)
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = migration_table("users", manager.get_database_backend())?;
        crate::database::dialect::drop_column(manager, table, TbUsers::RememberToken).await
    }
}
