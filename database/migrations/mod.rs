use openrust::database::migrations::MigrationEntry;

include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

// 入口保留在正式源码中，生成文件只提供自动发现的模块和清单数据。
pub(crate) const fn entries() -> &'static [MigrationEntry] {
    include!(concat!(env!("OUT_DIR"), "/migration_entries.rs"))
}
