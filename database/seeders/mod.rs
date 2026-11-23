use openrust::database::seeder::SeederEntry;

include!(concat!(env!("OUT_DIR"), "/seeders.rs"));

// 入口保留在正式源码中，生成文件只提供自动发现的模块和清单数据。
pub(crate) const fn entries() -> &'static [SeederEntry] {
    include!(concat!(env!("OUT_DIR"), "/seeder_entries.rs"))
}
