use std::{
    io,
    path::{Path, PathBuf},
};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{Stub, snake_case, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(SeederMakeCommand(CommandDefinition::new(
        Syntax::new("make:seeder")
            .about("Create a new seeder class")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue)),
    ))))
}

struct SeederMakeCommand(CommandDefinition);

impl Command for SeederMakeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let args = input.matches();
            let name = args
                .get_one::<String>("name")
                .expect("name required by Clap");
            let root = app.base_path();
            let path = create(root, name, args.get_flag("force"))
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let path = path.strip_prefix(root).unwrap_or(&path);
            output.info(&format!(
                "Seeder [{}] created successfully.",
                path.display()
            ));
            output
                .stdout
                .push_str("Register the seeder in database/seeders/mod.rs and rebuild.\n");
            Ok(0)
        })
    }
}

fn create(root: &Path, name: &str, force: bool) -> io::Result<PathBuf> {
    let parts: Vec<_> = name.split(['/', '\\']).collect();
    if parts.iter().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Use a PascalCase seeder name, such as UserSeeder.",
        ));
    }
    let (class, namespaces) = parts
        .split_last()
        .expect("split returns at least one component");
    let mut directory = root.join("database/seeders");
    for namespace in namespaces {
        directory.push(snake_case(namespace));
    }
    let path = directory.join(format!("{}.rs", snake_case(class)));
    let code = Stub::Seeder.load(root)?.replace("{{class}}", class);
    write_generated(&path, code.as_bytes(), force)?;
    Ok(path)
}
