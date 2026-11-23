use std::{
    fmt::Write as _,
    fs,
    io::{ErrorKind, IsTerminal as _, Write as _},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Arg, ArgAction, ArgMatches};
use zeroize::Zeroizing;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    encryption::{Cipher, Encrypter},
    foundation::Application,
};

mod readable;

#[derive(Clone, Copy)]
enum Operation {
    Encrypt,
    Decrypt,
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let flag = |name: &'static str, text| {
        Arg::new(name)
            .long(name)
            .help(text)
            .action(ArgAction::SetTrue)
    };
    for (operation, name, description) in [
        (
            Operation::Encrypt,
            "env:encrypt",
            "Encrypt an environment file",
        ),
        (
            Operation::Decrypt,
            "env:decrypt",
            "Decrypt an environment file",
        ),
    ] {
        let syntax = clap::Command::new(name)
            .about(description)
            .arg(Arg::new("key").long("key").help("The encryption key"))
            .arg(
                Arg::new("cipher")
                    .long("cipher")
                    .default_value("AES-256-CBC")
                    .help("The encryption cipher"),
            )
            .arg(flag("force", "Replace an existing destination file"));
        let syntax = match operation {
            Operation::Encrypt => syntax
                .arg(flag(
                    "readable",
                    "Keep variable names readable and preserve unchanged encrypted values",
                ))
                .arg(flag(
                    "prune",
                    "Remove the plaintext environment file after encryption",
                )),
            Operation::Decrypt => syntax
                .arg(
                    Arg::new("path")
                        .long("path")
                        .help("Directory for the decrypted file"),
                )
                .arg(
                    Arg::new("filename")
                        .long("filename")
                        .help("Name of the decrypted file"),
                ),
        };
        artisan.add_command(Box::new(EnvironmentEncryption {
            operation,
            definition: CommandDefinition::new(syntax),
        }))?;
    }
    Ok(())
}

struct EnvironmentEncryption {
    operation: Operation,
    definition: CommandDefinition,
}

impl Command for EnvironmentEncryption {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let matches = input.matches().clone();
            let cipher_name = value(&matches, "cipher").expect("cipher has a default");
            let cipher: Cipher = cipher_name.parse().map_err(error)?;
            let name = matches
                .get_many::<String>("env")
                .and_then(|mut values| values.next());
            let default_name =
                name.map_or_else(|| ".env".to_owned(), |name| format!(".env.{name}"));
            let source = if name.is_some() {
                app.base_path().join(&default_name)
            } else {
                app.container()
                    .get::<PathBuf>("env.file")
                    .map_err(error)?
                    .as_ref()
                    .clone()
            };
            let encrypted = source.with_file_name(format!(
                "{}.encrypted",
                source
                    .file_name()
                    .expect("environment filename")
                    .to_string_lossy(),
            ));
            let interactive = output.streams_to_terminal()
                && !matches.get_flag("no-interaction")
                && std::io::stdin().is_terminal()
                && std::io::stderr().is_terminal();

            match self.operation {
                Operation::Encrypt => {
                    let report_path = encrypted.clone();
                    let cipher_name = cipher_name.to_owned();
                    let generated = tokio::task::spawn_blocking(move || {
                        encrypt(source, encrypted, &matches, cipher, interactive)
                    })
                    .await
                    .map_err(error)??;
                    output.info("Environment successfully encrypted.");
                    if let Some(key) = generated {
                        let encoded = Zeroizing::new(STANDARD.encode(&*key));
                        writeln!(output.stdout, "Key: base64:{}", encoded.as_str())
                            .expect("writing to String");
                    }
                    writeln!(
                        output.stdout,
                        "Cipher: {cipher_name}\nEncrypted file: {}",
                        report_path.display()
                    )
                    .expect("writing to String");
                }
                Operation::Decrypt => {
                    let directory = value(&matches, "path").map_or(app.base_path(), Path::new);
                    let filename = value(&matches, "filename").unwrap_or(&default_name);
                    let mut components = Path::new(filename).components();
                    if filename.ends_with(".encrypted")
                        || !matches!(components.next(), Some(Component::Normal(part)) if part == filename)
                        || components.next().is_some()
                    {
                        return Err(ConsoleError::new("Invalid filename."));
                    }
                    let destination = directory.join(filename);
                    let report_path = destination.clone();
                    tokio::task::spawn_blocking(move || {
                        decrypt(encrypted, destination, &matches, cipher, interactive)
                    })
                    .await
                    .map_err(error)??;
                    output.info("Environment successfully decrypted.");
                    writeln!(output.stdout, "Decrypted file: {}", report_path.display())
                        .expect("writing to String");
                }
            }
            Ok(0)
        })
    }
}

fn encrypt(
    source: PathBuf,
    encrypted: PathBuf,
    matches: &ArgMatches,
    cipher: Cipher,
    interactive: bool,
) -> Result<Option<Zeroizing<Vec<u8>>>, ConsoleError> {
    let contents = Zeroizing::new(read(&source, "Environment file not found.")?);
    let exists = encrypted.try_exists().map_err(error)?;
    let force = matches.get_flag("force");
    let readable = matches.get_flag("readable");
    let preserve = readable && exists && !force;
    if exists && !force && !preserve {
        return Err(ConsoleError::new(
            "Encrypted environment file already exists.",
        ));
    }

    let mut supplied = value(matches, "key").map(|key| Zeroizing::new(key.to_owned()));
    if supplied.is_none() && interactive {
        if preserve {
            supplied = Some(secret("What is the encryption key?")?);
        } else {
            let terminal = console::Term::stderr();
            terminal
                .write_line("Encryption key: [g] generate or [k] enter a key (default: g)")
                .map_err(error)?;
            if terminal
                .read_line()
                .map_err(error)?
                .trim()
                .eq_ignore_ascii_case("k")
            {
                supplied = Some(secret("What is the encryption key?")?);
            }
        }
    }
    if preserve && supplied.as_ref().is_none_or(|key| key.is_empty()) {
        return Err(ConsoleError::new(
            "The existing encryption key is required to update the encrypted environment file.",
        ));
    }
    let (key, generated) = match supplied {
        Some(key) => (decode_key(&key)?, false),
        None if matches.get_flag("quiet") || matches.get_flag("silent") => {
            return Err(ConsoleError::new(
                "Supply --key when command output is suppressed.",
            ));
        }
        None => (
            Zeroizing::new(Encrypter::generate_key(cipher).map_err(error)?),
            true,
        ),
    };
    let encrypter = Encrypter::new(&*key, cipher).map_err(error)?;
    let previous = preserve
        .then(|| read(&encrypted, "Encrypted environment file not found."))
        .transpose()?;
    let payload = if readable {
        let contents = std::str::from_utf8(&contents)
            .map_err(|_| ConsoleError::new("Readable environment files must be UTF-8."))?;
        let previous = previous
            .as_deref()
            .map(std::str::from_utf8)
            .transpose()
            .map_err(|_| ConsoleError::new("Invalid readable encrypted environment file."))?;
        readable::encrypt(contents, previous, &encrypter)?
    } else {
        encrypter.encrypt(&contents, true).map_err(error)?
    };
    if previous.as_deref() != Some(payload.as_bytes()) {
        write_private(&encrypted, payload.as_bytes(), force || preserve)?;
    }
    if matches.get_flag("prune") {
        let current = Zeroizing::new(read(&source, "Environment file not found.")?);
        if current.as_slice() != contents.as_slice() {
            return Err(ConsoleError::new(
                "Environment file changed during encryption and was kept.",
            ));
        }
        fs::remove_file(source).map_err(error)?;
    }
    Ok(generated.then_some(key))
}

fn decrypt(
    encrypted: PathBuf,
    destination: PathBuf,
    matches: &ArgMatches,
    cipher: Cipher,
    interactive: bool,
) -> Result<(), ConsoleError> {
    let mut key = value(matches, "key")
        .filter(|key| !key.is_empty())
        .map(|key| Zeroizing::new(key.to_owned()))
        .or_else(|| {
            std::env::var("ENV_ENCRYPTION_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .map(Zeroizing::new)
        });
    if key.is_none() && interactive {
        key = Some(secret("What is the decryption key?")?);
    }
    let key = key
        .filter(|key| !key.is_empty())
        .ok_or_else(|| ConsoleError::new("A decryption key is required."))?;
    let contents = read(&encrypted, "Encrypted environment file not found.")?;
    let force = matches.get_flag("force");
    if !force && destination.try_exists().map_err(error)? {
        return Err(ConsoleError::new("Environment file already exists."));
    }
    let key = decode_key(&key)?;
    let encrypter = Encrypter::new(&*key, cipher).map_err(error)?;
    let contents = std::str::from_utf8(&contents)
        .map_err(|_| ConsoleError::new("Invalid encrypted environment file."))?;
    let plaintext = if Encrypter::appears_encrypted(contents) {
        Zeroizing::new(encrypter.decrypt(contents, true).map_err(error)?)
    } else {
        readable::decrypt(contents, &encrypter)?
    };
    write_private(&destination, &plaintext, force)
}

fn value<'a>(matches: &'a ArgMatches, name: &str) -> Option<&'a str> {
    matches.get_one::<String>(name).map(String::as_str)
}

fn decode_key(key: &str) -> Result<Zeroizing<Vec<u8>>, ConsoleError> {
    match key.strip_prefix("base64:") {
        Some(encoded) => STANDARD
            .decode(encoded)
            .map(Zeroizing::new)
            .map_err(|_| ConsoleError::new("Invalid base64 encryption key.")),
        None => Ok(Zeroizing::new(key.as_bytes().to_vec())),
    }
}

fn secret(prompt: &str) -> Result<Zeroizing<String>, ConsoleError> {
    let terminal = console::Term::stderr();
    terminal.write_line(prompt).map_err(error)?;
    terminal
        .read_secure_line()
        .map(Zeroizing::new)
        .map_err(error)
}

fn read(path: &Path, missing: &str) -> Result<Vec<u8>, ConsoleError> {
    fs::read(path).map_err(|source| {
        if source.kind() == ErrorKind::NotFound {
            ConsoleError::new(missing)
        } else {
            error(source)
        }
    })
}

fn write_private(path: &Path, contents: &[u8], replace: bool) -> Result<(), ConsoleError> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    temporary.write_all(contents).map_err(error)?;
    temporary.as_file().sync_all().map_err(error)?;
    if replace {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .map_err(error)?;
    Ok(())
}

fn error(source: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(source.to_string())
}
