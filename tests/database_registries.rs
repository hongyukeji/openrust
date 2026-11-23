use openrust::database::{migrations::MigrationEntry, seeder::SeederEntry};

#[path = "../app/models/mod.rs"]
pub(crate) mod models;
mod app {
    pub(crate) use crate::models;
}
#[path = "../database/mod.rs"]
mod database;

// 通过同级调用方和常量上下文验证装配入口，不依赖生成文件中的函数声明。
mod bootstrap_consumer {
    use super::*;

    pub const MIGRATIONS: &[MigrationEntry] = database::migrations::entries();
    pub const SEEDERS: &[SeederEntry] = database::seeders::entries();
}

#[test]
fn migration_registry_is_accessible_from_a_sibling_and_matches_discovered_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let entries = bootstrap_consumer::MIGRATIONS;
    assert!(
        entries
            .iter()
            .any(|entry| entry.path.ends_with("create_users_table.rs"))
    );
    for entry in entries {
        assert!(root.join(entry.path).is_file(), "{}", entry.path);
    }
    let expected = std::fs::read_dir(root.join("database/migrations"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|file| {
            file.path().extension().is_some_and(|ext| ext == "rs")
                && file
                    .file_name()
                    .to_string_lossy()
                    .starts_with(|ch: char| ch.is_ascii_digit())
        })
        .count();
    assert_eq!(entries.len(), expected);
    assert_eq!(
        entries.iter().map(|entry| entry.path).collect::<Vec<_>>(),
        database::migrations::entries()
            .iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn seeder_registry_is_accessible_from_a_sibling_in_const_and_runtime_contexts() {
    let entries = bootstrap_consumer::SEEDERS;
    assert!(!entries.is_empty());
    assert_eq!(entries.len(), database::seeders::entries().len());
}
