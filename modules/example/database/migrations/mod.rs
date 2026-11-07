//! 仅属于 `example` 模块的业务数据库迁移。

use openrust::database::sea_orm_migration::prelude::*;
pub(crate) struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(CreateExampleHeartbeats),
            Box::new(CreateExampleArticles),
            Box::new(CreateExampleArticleEvents),
            Box::new(AddExampleArticlePrivateNotes),
            Box::new(CreateExampleAttachmentCleanup),
            Box::new(CreateExampleArticleImports),
            Box::new(CreateExampleOidcBindIntents),
            Box::new(CreateExampleTenantRecords),
            Box::new(CreateExampleRecordShowcase),
            Box::new(CreateExampleRecordCategoryLinks),
        ]
    }
}

/// 示例记录与补充分类的多对多中间表。
struct CreateExampleRecordCategoryLinks;

impl MigrationName for CreateExampleRecordCategoryLinks {
    fn name(&self) -> &'static str {
        "m20261107_000010_example_record_category_links"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleRecordCategoryLinks {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        use ExampleRecordCategoryLinks as Links;
        manager
            .create_table(
                Table::create()
                    .table(Links::Table)
                    .col(ColumnDef::new(Links::RecordId).big_integer().not_null())
                    .col(ColumnDef::new(Links::CategoryId).big_integer().not_null())
                    .col(ColumnDef::new(Links::Label).string_len(100).not_null())
                    .primary_key(Index::create().col(Links::RecordId).col(Links::CategoryId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_example_category_links_record")
                            .from(Links::Table, Links::RecordId)
                            .to(ExampleRecords::Table, ExampleRecords::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_example_category_links_category")
                            .from(Links::Table, Links::CategoryId)
                            .to(ExampleRecordCategories::Table, ExampleRecordCategories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ExampleRecordCategoryLinks::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ExampleRecordCategoryLinks {
    Table,
    RecordId,
    CategoryId,
    Label,
}

/// 独立的字段类型示例表，不与文章、用户等框架业务实体共用。
struct CreateExampleRecordShowcase;

impl MigrationName for CreateExampleRecordShowcase {
    fn name(&self) -> &'static str {
        "m20261029_000009_example_record_showcase"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleRecordShowcase {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleRecordCategories::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleRecordCategories::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecordCategories::Name)
                            .string_len(100)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ExampleRecords::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleRecords::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::Name)
                            .string_len(160)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::Description)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::Priority)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::ViewCount)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ExampleRecords::Enabled).boolean().not_null())
                    .col(
                        openrust::database::dialect::timestamp_utc(
                            ExampleRecords::PublishedAt,
                            manager.get_database_backend(),
                        )?
                        .null(),
                    )
                    .col(
                        openrust::database::dialect::json(
                            ExampleRecords::Metadata,
                            manager.get_database_backend(),
                        )?
                        .null(),
                    )
                    .col(
                        ColumnDef::new(ExampleRecords::CategoryId)
                            .big_integer()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_example_records_category_id")
                            .from(ExampleRecords::Table, ExampleRecords::CategoryId)
                            .to(ExampleRecordCategories::Table, ExampleRecordCategories::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_example_records_owner_id")
                    .table(ExampleRecords::Table)
                    .col(ExampleRecords::OwnerId)
                    .col(ExampleRecords::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleRecords::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ExampleRecordCategories::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ExampleRecordCategories {
    Table,
    Id,
    Name,
}

#[derive(Iden)]
enum ExampleRecords {
    Table,
    Id,
    OwnerId,
    Name,
    Description,
    Priority,
    ViewCount,
    Enabled,
    PublishedAt,
    Metadata,
    CategoryId,
}

struct CreateExampleHeartbeats;

impl MigrationName for CreateExampleHeartbeats {
    fn name(&self) -> &'static str {
        "m20261028_000001_example_heartbeats"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleHeartbeats {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleHeartbeats::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleHeartbeats::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleHeartbeats::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ExampleHeartbeats {
    Table,
    Id,
}

struct CreateExampleArticles;

impl MigrationName for CreateExampleArticles {
    fn name(&self) -> &'static str {
        "m20261028_000002_example_articles"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleArticles {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleArticles::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleArticles::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticles::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticles::Title)
                            .string_len(160)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ExampleArticles::Body).text().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_example_articles_owner_id")
                    .table(ExampleArticles::Table)
                    .col(ExampleArticles::OwnerId)
                    .col(ExampleArticles::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleArticles::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ExampleArticles {
    Table,
    Id,
    OwnerId,
    Title,
    Body,
    PrivateNoteCiphertext,
}

struct CreateExampleArticleEvents;

impl MigrationName for CreateExampleArticleEvents {
    fn name(&self) -> &'static str {
        "m20261028_000003_example_article_events"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleArticleEvents {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleArticleEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleArticleEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleEvents::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleEvents::ArticleId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleEvents::Action)
                            .string_len(16)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleArticleEvents::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ExampleArticleEvents {
    Table,
    Id,
    OwnerId,
    ArticleId,
    Action,
}

struct AddExampleArticlePrivateNotes;

impl MigrationName for AddExampleArticlePrivateNotes {
    fn name(&self) -> &'static str {
        "m20261028_000004_example_article_private_notes"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for AddExampleArticlePrivateNotes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ExampleArticles::Table)
                    .add_column(
                        ColumnDef::new(ExampleArticles::PrivateNoteCiphertext)
                            .text()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        openrust::database::dialect::drop_column(
            manager,
            ExampleArticles::Table,
            ExampleArticles::PrivateNoteCiphertext,
        )
        .await
    }
}

struct CreateExampleAttachmentCleanup;

impl MigrationName for CreateExampleAttachmentCleanup {
    fn name(&self) -> &'static str {
        "m20261028_000005_example_attachment_cleanup"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleAttachmentCleanup {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleAttachmentCleanup::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleAttachmentCleanup::ArticleId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleAttachmentCleanup::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleAttachmentCleanup::TenantId)
                            .string_len(128)
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ExampleAttachmentCleanup::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ExampleAttachmentCleanup {
    Table,
    ArticleId,
    OwnerId,
    TenantId,
}

struct CreateExampleArticleImports;

impl MigrationName for CreateExampleArticleImports {
    fn name(&self) -> &'static str {
        "m20261028_000006_example_article_imports"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleArticleImports {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleArticleImports::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleArticleImports::ImportId)
                            .string_len(36)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::SourceKey)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::Format)
                            .string_len(4)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::State)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::ErrorCode)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::CreatedAtMs)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::TerminalAtMs)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ExampleArticleImports::SourceRemoved)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleArticleImports::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ExampleArticleImports {
    Table,
    ImportId,
    OwnerId,
    SourceKey,
    Format,
    State,
    ErrorCode,
    CreatedAtMs,
    TerminalAtMs,
    SourceRemoved,
}

struct CreateExampleOidcBindIntents;

impl MigrationName for CreateExampleOidcBindIntents {
    fn name(&self) -> &'static str {
        "m20261028_000007_example_oidc_bind_intents"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleOidcBindIntents {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleOidcBindIntents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleOidcBindIntents::StateHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ExampleOidcBindIntents::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleOidcBindIntents::BrowserBindingHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleOidcBindIntents::ExpiresAtMs)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleOidcBindIntents::Phase)
                            .string_len(16)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ExampleOidcBindIntents::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ExampleOidcBindIntents {
    Table,
    StateHash,
    OwnerId,
    BrowserBindingHash,
    ExpiresAtMs,
    Phase,
}

struct CreateExampleTenantRecords;

impl MigrationName for CreateExampleTenantRecords {
    fn name(&self) -> &'static str {
        "m20261029_000008_example_tenant_records"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleTenantRecords {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleTenantMemberships::Table)
                    .col(
                        ColumnDef::new(ExampleTenantMemberships::TenantId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleTenantMemberships::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ExampleTenantMemberships::TenantId)
                            .col(ExampleTenantMemberships::UserId),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ExampleTenantRecords::Table)
                    .col(
                        ColumnDef::new(ExampleTenantRecords::TenantId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleTenantRecords::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ExampleTenantRecords::Value)
                            .string_len(512)
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ExampleTenantRecords::TenantId)
                            .col(ExampleTenantRecords::OwnerId),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleTenantRecords::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ExampleTenantMemberships::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ExampleTenantMemberships {
    Table,
    TenantId,
    UserId,
}

#[derive(Iden)]
enum ExampleTenantRecords {
    Table,
    TenantId,
    OwnerId,
    Value,
}
