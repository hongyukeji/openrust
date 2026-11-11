use std::{
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

/// Creates a timestamped migration from a Rust stub.
pub struct MigrationCreator;

impl MigrationCreator {
    pub fn create(
        &self,
        name: &str,
        directory: &Path,
        table: Option<&str>,
        create: bool,
    ) -> io::Result<PathBuf> {
        self.create_with_body(name, directory, table, create, None)
    }

    pub(crate) fn create_with_body(
        &self,
        name: &str,
        directory: &Path,
        table: Option<&str>,
        create: bool,
        body: Option<(&str, &str)>,
    ) -> io::Result<PathBuf> {
        let name = snake_case(name);
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid migration name.",
            ));
        }
        let inferred_create = name
            .strip_prefix("create_")
            .and_then(|name| name.strip_suffix("_table"));
        let inferred_update = name
            .strip_prefix("add_")
            .and_then(|name| name.rsplit_once("_to_"))
            .or_else(|| {
                name.strip_prefix("remove_")
                    .and_then(|name| name.rsplit_once("_from_"))
            })
            .map(|(_, table)| table.strip_suffix("_table").unwrap_or(table));
        let create = create || (table.is_none() && inferred_create.is_some());
        let table = table.or(inferred_create).or(inferred_update);
        if create && table.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Specify the table to create.",
            ));
        }
        fs::create_dir_all(directory)?;
        let suffix = format!("_{name}.rs");
        for entry in fs::read_dir(directory)? {
            if entry?.file_name().to_string_lossy().get(17..) == Some(&suffix) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "A migration with this name already exists.",
                ));
            }
        }
        let path = directory.join(format!(
            "{}{suffix}",
            chrono::Utc::now().format("%Y_%m_%d_%H%M%S")
        ));
        let (up, down) = body.map(|(up, down)| (up.to_owned(), down.to_owned())).unwrap_or_else(|| match table {
            Some(table) if create => (
                format!(
                    "connection.getSchemaBuilder().create({table:?}, |table| {{\n                table.id();\n                table.timestamps();\n            }}).await"
                ),
                format!("connection.getSchemaBuilder().dropIfExists({table:?}).await"),
            ),
            Some(table) => (
                format!("connection.getSchemaBuilder().table({table:?}, |_table| {{}}).await"),
                format!("connection.getSchemaBuilder().table({table:?}, |_table| {{}}).await"),
            ),
            None => ("Ok(())".into(), "Ok(())".into()),
        });
        let code = format!(
            "use openrust::database::{{DatabaseFuture, DatabaseResult, migrations::{{Migration, MigrationContext}}}};\n\n\
             pub struct Definition;\n\nimpl Migration for Definition {{\n    \
             fn up<'a>(&'a self, _connection: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {{\n        \
             Box::pin(async move {{ {up} }})\n    }}\n\n    \
             fn down<'a>(&'a self, _connection: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {{\n        \
             Box::pin(async move {{ {down} }})\n    }}\n}}\n"
        );
        let code = if table.is_some() {
            code.replace("_connection:", "connection:")
        } else {
            code
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        if let Err(error) = file.write_all(code.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(path)
    }
}

fn snake_case(name: &str) -> String {
    let mut snake = String::new();
    let mut previous = false;
    for character in name.trim().chars() {
        if ((character.is_ascii_uppercase() && previous) || character == ' ' || character == '-')
            && !snake.ends_with('_')
        {
            snake.push('_');
        }
        if character != ' ' && character != '-' {
            snake.push(character.to_ascii_lowercase());
        }
        previous = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    snake.trim_matches('_').to_owned()
}
