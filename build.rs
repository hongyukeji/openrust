use std::{
    env,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

const COMMANDS: &[&str] = &["app/console/commands"];
const FACTORIES: &str = "database/factories";
const MIGRATIONS: &str = "database/migrations";
const SEEDERS: &str = "database/seeders";

fn main() -> io::Result<()> {
    // Windows 可执行文件需要为队列任务的原生入口保留足够栈空间。
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo::rustc-link-arg-bin=main=/STACK:16777216");
    }
    println!("cargo:rerun-if-changed={FACTORIES}");
    println!("cargo:rerun-if-changed={MIGRATIONS}");
    println!("cargo:rerun-if-changed={SEEDERS}");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output directory"));
    openrust_package_discovery::generate()?;
    generate_migrations(&root, &output)?;
    generate_seeders(&root, &output)?;
    generate_commands(&root, &output)?;
    fs::write(
        output.join("factories.rs"),
        factory_modules(&root.join(FACTORIES), 0)?,
    )
}

fn generate_commands(root: &Path, output: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    for directory in COMMANDS {
        println!("cargo:rerun-if-changed={directory}");
        let directory = root.join(directory);
        if directory.is_dir() {
            collect_commands(&directory, &mut files)?;
        }
    }
    files.sort_unstable();
    files.dedup();
    let mut modules = String::new();
    let mut entries = String::new();
    for (index, path) in files.iter().enumerate() {
        let source = source_path(path)?;
        writeln!(modules, "#[path = {source:?}]\nmod command_{index};").unwrap();
        writeln!(entries, "    command_{index}::COMMAND,").unwrap();
    }
    fs::write(output.join("commands.rs"), modules)?;
    fs::write(
        output.join("command_entries.rs"),
        format!("&[\n{entries}]\n"),
    )
}

fn collect_commands(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for file in sorted_entries(directory)? {
        let kind = file.file_type()?;
        let path = file.path();
        if kind.is_dir() {
            collect_commands(&path, files)?;
        } else if kind.is_file()
            && path.extension().is_some_and(|ext| ext == "rs")
            && file.file_name() != "mod.rs"
            && fs::read_to_string(&path).and_then(|text| command_entry(&path, &text))?
        {
            files.push(path);
        }
    }
    Ok(())
}

fn command_entry(path: &Path, text: &str) -> io::Result<bool> {
    // 只发现声明注册常量的文件；注释、嵌套常量和旧的手动模块保持原状。
    if !text.contains("COMMAND") {
        return Ok(false);
    }
    let file = syn::parse_file(text).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {error}", path.display()),
        )
    })?;
    Ok(file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Const(constant) if constant.ident == "COMMAND")))
}

fn generate_migrations(root: &Path, output: &Path) -> io::Result<()> {
    let mut modules = String::new();
    let mut entries = String::new();
    for file in sorted_entries(&root.join(MIGRATIONS))? {
        let path = file.path();
        let Some(stem) = migration_stem(&path) else {
            continue;
        };
        if !file.file_type()?.is_file() {
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

    fs::write(output.join("migrations.rs"), modules)?;
    fs::write(
        output.join("migration_entries.rs"),
        format!(
            "{{\n    static ENTRIES: &[MigrationEntry] = &[\n{entries}    ];\n    ENTRIES\n}}\n"
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
    fs::write(output.join("seeders.rs"), modules)?;
    fs::write(
        output.join("seeder_entries.rs"),
        format!("{{\n    static ENTRIES: &[SeederEntry] = &[\n{entries}    ];\n    ENTRIES\n}}\n"),
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
