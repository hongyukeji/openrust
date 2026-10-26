//! 默认认证器使用的数据库 schema。

use crate::db::{
    DatabaseTableNames, dialect,
    sea_orm::{DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

fn migration_table(logical: &str, backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "OpenRust migration requires MigrationPlugin or TestDatabase table-name scope".into(),
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
    EmailVerifiedAt,
    DisabledAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TbSessions {
    IdHash,
    Data,
    ExpiresAt,
    CreatedAt,
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
pub struct AuthMigration;

/// 创建 refresh token 家族的追加式 schema migration。
pub struct AuthRefreshTokenMigration;

/// 创建共享的一次性验证码表。
pub struct AuthVerificationCodeMigration;

/// 创建 [`crate::auth::Policy`] 使用的追加式角色与权限 schema。
pub struct AuthPolicyMigration;

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
        let sessions_table = migration_table("sessions", backend)?;
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
        manager
            .create_table(
                Table::create()
                    .table(sessions_table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TbSessions::IdHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(dialect::json(TbSessions::Data, backend)?)
                    .col(dialect::timestamp_utc(TbSessions::ExpiresAt, backend)?)
                    .col(dialect::timestamp_utc(TbSessions::CreatedAt, backend)?)
                    .to_owned(),
            )
            .await?;
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
        let sessions_table = migration_table("sessions", backend)?;
        let users_table = migration_table("users", backend)?;
        manager
            .drop_table(Table::drop().table(pat_table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(sessions_table).if_exists().to_owned())
            .await?;
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
