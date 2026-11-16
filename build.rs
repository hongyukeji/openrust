use std::{
    env,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

const FACTORIES: &str = "database/factories";
const MIGRATIONS: &str = "database/migrations";
const SEEDERS: &str = "database/seeders";
const SANCTUM_MIGRATION: &str = "0001_01_01_000012_create_personal_access_tokens_table";

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed={FACTORIES}");
    println!("cargo:rerun-if-changed={MIGRATIONS}");
    println!("cargo:rerun-if-changed={SEEDERS}");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output directory"));
    generate_migrations(&root, &output)?;
    generate_seeders(&root, &output)?;
    fs::write(
        output.join("factories.rs"),
        factory_modules(&root.join(FACTORIES), 0)?,
    )
}

fn generate_migrations(root: &Path, output: &Path) -> io::Result<()> {
    let mut modules = String::new();
    let mut entries = String::new();
    for file in sorted_entries(&root.join(MIGRATIONS))? {
        let path = file.path();
        let Some(stem) = migration_stem(&path) else {
            continue;
        };
        if !file.file_type()?.is_file() || (stem == SANCTUM_MIGRATION && !cfg!(feature = "sanctum"))
        {
            continue;
        }

        let relative = format!("{MIGRATIONS}/{stem}.rs");
        let source = source_path(&path)?;
        writeln!(modules, "#[path = {source:?}]\nmod migration_{stem};").unwrap();
        writeln!(
            entries,
            "        MigrationEntry::new({relative:?}, &migration_{stem}::Definition),"
        )
        .unwrap();
    }

    fs::write(
        output.join("migrations.rs"),
        format!(
            "{modules}\npub(crate) const fn entries() -> &'static [MigrationEntry] {{\n    static ENTRIES: &[MigrationEntry] = &[\n{entries}    ];\n    ENTRIES\n}}\n"
        ),
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

fn generate_seeders(root: &Path, output: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    collect_seeders(&root.join(SEEDERS), &mut files)?;
    let mut modules = String::new();
    let mut entries = String::new();
    for (index, path) in files.iter().enumerate() {
        let source = source_path(path)?;
        writeln!(modules, "#[path = {source:?}]\nmod seeder_{index};").unwrap();
        writeln!(entries, "        seeder_{index}::ENTRY,").unwrap();
    }
    fs::write(
        output.join("seeders.rs"),
        format!(
            "{modules}\npub(crate) const fn entries() -> &'static [SeederEntry] {{\n    static ENTRIES: &[SeederEntry] = &[\n{entries}    ];\n    ENTRIES\n}}\n"
        ),
    )
}

fn collect_seeders(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for file in sorted_entries(directory)? {
        let path = file.path();
        let kind = file.file_type()?;
        if kind.is_dir() {
            collect_seeders(&path, files)?;
        } else if kind.is_file()
            && path.extension().is_some_and(|ext| ext == "rs")
            && file.file_name() != "mod.rs"
        {
            files.push(path);
        }
    }
    Ok(())
}

fn factory_modules(directory: &Path, depth: usize) -> io::Result<String> {
    let mut modules = String::new();
    let indent = "    ".repeat(depth);
    for file in sorted_entries(directory)? {
        let path = file.path();
        let kind = file.file_type()?;
        let name = file.file_name();
        let name = Path::new(&name);
        if kind.is_dir() {
            let children = factory_modules(&path, depth + 1)?;
            if !children.is_empty() {
                let module = source_path(name)?;
                writeln!(
                    modules,
                    "{indent}pub(crate) mod r#{module} {{\n{children}{indent}}}"
                )
                .unwrap();
            }
        } else if kind.is_file()
            && name.extension().is_some_and(|ext| ext == "rs")
            && name != Path::new("mod.rs")
        {
            let module = source_path(Path::new(name.file_stem().expect("Rust file stem")))?;
            let source = source_path(&path)?;
            writeln!(
                modules,
                "{indent}#[path = {source:?}]\n{indent}pub(crate) mod r#{module};"
            )
            .unwrap();
        }
    }
    Ok(modules)
}

fn sorted_entries(directory: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut files = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    files.sort_unstable_by_key(fs::DirEntry::file_name);
    Ok(files)
}

fn source_path(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Source path must be UTF-8: {}", path.display()),
        )
    })
}
