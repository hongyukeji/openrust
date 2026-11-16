use std::{
    env,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

const DIRECTORY: &str = "database/migrations";
const SANCTUM_MIGRATION: &str = "0001_01_01_000012_create_personal_access_tokens_table";

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed={DIRECTORY}");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let mut files = fs::read_dir(root.join(DIRECTORY))?.collect::<io::Result<Vec<_>>>()?;
    files.sort_unstable_by_key(fs::DirEntry::file_name);

    let mut modules = String::new();
    let mut entries = String::new();
    for file in files {
        let path = file.path();
        let Some(stem) = migration_stem(&path) else {
            continue;
        };
        if !file.file_type()?.is_file() || (stem == SANCTUM_MIGRATION && !cfg!(feature = "sanctum"))
        {
            continue;
        }

        let relative = format!("{DIRECTORY}/{stem}.rs");
        let source = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Migration path must be UTF-8")
        })?;
        writeln!(modules, "#[path = {source:?}]\nmod migration_{stem};").unwrap();
        writeln!(
            entries,
            "    MigrationEntry::new({relative:?}, &migration_{stem}::Definition),"
        )
        .unwrap();
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output directory"));
    fs::write(
        output.join("migrations.rs"),
        format!("{modules}\npub(crate) const ENTRIES: &[MigrationEntry] = &[\n{entries}];\n"),
    )
}

fn migration_stem(path: &Path) -> Option<&str> {
    if path.extension()? != "rs" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let mut parts = stem.splitn(5, '_');
    for width in [4, 2, 2, 6] {
        let part = parts.next()?;
        if part.len() != width || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    let name = parts.next()?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
    .then_some(stem)
}
