use crate::{KeyError, Passport};
use aws_lc_rs::{
    encoding::{AsDer, Pkcs8V1Der, PublicKeyX509Der},
    rsa::{KeyPair, KeySize},
    signature::KeyPair as _,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use zeroize::Zeroizing;

pub struct KeysCommand {
    definition: CommandDefinition,
}
impl Default for KeysCommand {
    fn default() -> Self {
        Self {
            definition: CommandDefinition::new(
                Syntax::new("passport:keys")
                    .about("Create the encryption keys for API authentication")
                    .arg(
                        Arg::new("force")
                            .long("force")
                            .action(ArgAction::SetTrue)
                            .help("Overwrite keys they already exist"),
                    )
                    .arg(
                        Arg::new("length")
                            .long("length")
                            .default_value("4096")
                            .allow_hyphen_values(true)
                            .help("The length of the private key"),
                    ),
            ),
        }
    }
}
impl Command for KeysCommand {
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
            let passport = app.make::<Passport>().await.map_err(error)?;
            let public = passport.keyPath("oauth-public.key");
            let private = passport.keyPath("oauth-private.key");
            let force = input.matches().get_flag("force");
            if !force
                && (public.try_exists().map_err(error)? || private.try_exists().map_err(error)?)
            {
                output.error(&KeyError::Exists.to_string());
                return Ok(1);
            }
            let length = input
                .matches()
                .get_one::<String>("length")
                .expect("default length")
                .parse::<usize>()
                .map_err(|_| error(KeyError::Generation))?;
            tokio::task::spawn_blocking(move || generate_keys(&public, &private, length, force))
                .await
                .map_err(|_| error(KeyError::Generation))?
                .map_err(error)?;
            output.info("Encryption keys generated successfully.");
            Ok(0)
        })
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
fn pem(label: &str, der: &[u8]) -> Zeroizing<String> {
    let encoded = Zeroizing::new(STANDARD.encode(der));
    let mut value = Zeroizing::new(format!("-----BEGIN {label}-----\n"));
    for chunk in encoded.as_bytes().chunks(64) {
        value.push_str(std::str::from_utf8(chunk).expect("base64 ASCII"));
        value.push('\n');
    }
    value.push_str(&format!("-----END {label}-----\n"));
    value
}
fn generate(length: usize) -> Result<(Zeroizing<String>, Zeroizing<String>), KeyError> {
    let native = match length {
        2048 => Some(KeySize::Rsa2048),
        3072 => Some(KeySize::Rsa3072),
        4096 => Some(KeySize::Rsa4096),
        8192 => Some(KeySize::Rsa8192),
        _ => None,
    };
    if let Some(size) = native {
        let key = KeyPair::generate(size).map_err(|_| KeyError::Generation)?;
        let private: Pkcs8V1Der<'static> = key.as_der().map_err(|_| KeyError::Generation)?;
        let public: PublicKeyX509Der<'static> = key
            .public_key()
            .as_der()
            .map_err(|_| KeyError::Generation)?;
        return Ok((
            pem("PUBLIC KEY", public.as_ref()),
            pem("PRIVATE KEY", private.as_ref()),
        ));
    }
    // Only key generation / PEM parsing uses RustCrypto. No RSA decrypt/sign primitive
    // from this library is exposed or used by the online OAuth server.
    let key =
        RsaPrivateKey::new(&mut rand::rngs::OsRng, length).map_err(|_| KeyError::Generation)?;
    let public = RsaPublicKey::from(&key)
        .to_public_key_pem(LineEnding::LF)
        .map_err(|_| KeyError::Generation)?;
    let private = key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|_| KeyError::Generation)?;
    Ok((Zeroizing::new(public), private))
}
fn write_key(path: &Path, value: &[u8], force: bool, mode: u32) -> Result<(), KeyError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.is_file()
    {
        return Err(KeyError::Write);
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(path).map_err(|_| KeyError::Write)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| KeyError::Write)?;
    }
    file.write_all(value)
        .and_then(|_| file.sync_all())
        .map_err(|_| KeyError::Write)
}

pub(super) fn generate_keys(
    public: &Path,
    private: &Path,
    length: usize,
    force: bool,
) -> Result<(), KeyError> {
    for path in [public, private] {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && !metadata.is_file()
        {
            return Err(KeyError::Write);
        }
    }
    let (public_key, private_key) = generate(length)?;
    write_key(public, public_key.as_bytes(), force, 0o660)?;
    write_key(private, private_key.as_bytes(), force, 0o600)?;
    Ok::<_, KeyError>(())
}
