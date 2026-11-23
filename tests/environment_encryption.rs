use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use openrust::encryption::{Cipher, Encrypter};

const KEY: &str = "0123456789abcdef0123456789abcdef";
const WRONG: &str = "ffffffffffffffffffffffffffffffff";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "openrust-env-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn artisan(root: &Path, arguments: &[&str], key: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
    command
        .arg("artisan")
        .args(arguments)
        .env("APP_BASE_PATH", root)
        .env_remove("APP_ENV")
        .env_remove("ENV_ENCRYPTION_KEY");
    if let Some(key) = key {
        command.env("ENV_ENCRYPTION_KEY", key);
    }
    command.output().unwrap()
}

fn success(result: Output) -> String {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn failure(result: Output, message: &str) {
    assert_eq!(result.status.code(), Some(1));
    let output = String::from_utf8_lossy(&result.stderr);
    assert!(output.contains(message), "{output}");
    assert!(!output.contains(KEY));
    assert!(!output.contains(WRONG));
}

#[test]
fn artisan_encrypts_and_decrypts_an_environment_file_byte_for_byte() {
    let root = TestRoot::new();
    let plain = root.path().join(".env.staging");
    let encrypted = root.path().join(".env.staging.encrypted");
    let contents = b"# preserve comment and line endings\r\nAPP_NAME=Staging\r\nPORT_PROBE=127\r\n";
    fs::write(&plain, contents).unwrap();

    let generated = success(artisan(
        root.path(),
        &["env:encrypt", "--env=staging", "--prune", "-n"],
        None,
    ));
    let key = generated
        .lines()
        .find_map(|line| line.strip_prefix("Key: "))
        .unwrap();
    assert!(key.starts_with("base64:"));
    let payload = fs::read_to_string(&encrypted).unwrap();
    assert!(Encrypter::appearsEncrypted(&payload));
    assert!(!plain.exists());

    failure(
        artisan(
            root.path(),
            &["env:decrypt", "--env", "staging", "-n"],
            None,
        ),
        "A decryption key is required",
    );
    failure(
        artisan(
            root.path(),
            &["env:decrypt", "--env", "staging", "--key", WRONG],
            None,
        ),
        "Could not decrypt the data",
    );
    assert!(!plain.exists());

    success(artisan(
        root.path(),
        &["env:decrypt", "--env", "staging"],
        Some(key),
    ));
    assert_eq!(fs::read(&plain).unwrap(), contents);
    failure(
        artisan(root.path(), &["env:decrypt", "--env", "staging"], Some(key)),
        "Environment file already exists",
    );

    let output = root.path().join("restore");
    fs::create_dir(&output).unwrap();
    success(artisan(
        root.path(),
        &[
            "env:decrypt",
            "--env=staging",
            "--path",
            output.to_str().unwrap(),
            "--filename",
            "restore.env",
        ],
        Some(key),
    ));
    assert_eq!(fs::read(output.join("restore.env")).unwrap(), contents);
    failure(
        artisan(
            root.path(),
            &[
                "env:decrypt",
                "--env=staging",
                "--filename",
                ".env.staging.encrypted",
                "--force",
            ],
            Some(key),
        ),
        "Invalid filename",
    );
    failure(
        artisan(
            root.path(),
            &[
                "env:decrypt",
                "--env=staging",
                "--filename",
                "../outside",
                "--force",
            ],
            Some(key),
        ),
        "Invalid filename",
    );
    assert_eq!(fs::read_to_string(&encrypted).unwrap(), payload);

    fs::write(&plain, "APP_NAME=Different\n").unwrap();
    success(artisan(
        root.path(),
        &["env:decrypt", "--env=staging", "--force"],
        Some(key),
    ));
    assert_eq!(fs::read(&plain).unwrap(), contents);
    failure(
        artisan(
            root.path(),
            &["env:encrypt", "--env=staging", "--cipher", "unrecognized"],
            None,
        ),
        "Unsupported",
    );
    failure(
        artisan(
            root.path(),
            &["env:encrypt", "--env=missing", "--key", KEY],
            None,
        ),
        "Environment file not found",
    );
}

#[test]
fn artisan_preserves_unchanged_readable_values_and_rejects_corruption_before_pruning() {
    let root = TestRoot::new();
    let source = root.path().join(".env");
    let encrypted = root.path().join(".env.encrypted");
    let original = "# comment\r\nA=first\r\nA=second\r\nB=\"line one\r\nline two\"\r\nREF=${A}\r\n";
    fs::write(&source, original).unwrap();
    let output = success(artisan(
        root.path(),
        &["env:encrypt", "--readable", "--key", KEY],
        None,
    ));
    assert!(!output.contains(KEY));
    let before = fs::read_to_string(&encrypted).unwrap();
    let lines: Vec<_> = before.lines().collect();
    assert_eq!(lines.len(), 4);
    for name in ["A=", "B=", "REF="] {
        assert!(before.lines().any(|line| line.starts_with(name)));
    }

    let preserved = format!("# preserved\r\n{}", before.replace('\n', "\r\n"));
    fs::write(&encrypted, &preserved).unwrap();
    success(artisan(
        root.path(),
        &["env:encrypt", "--readable", "--key", KEY],
        None,
    ));
    assert_eq!(fs::read_to_string(&encrypted).unwrap(), preserved);

    let edited = "A=second\nA=third\nB=\"line one\nline two\"\nREF=${A}\n";
    fs::write(&source, edited).unwrap();
    failure(
        artisan(
            root.path(),
            &["env:encrypt", "--readable", "-n", "--prune"],
            None,
        ),
        "existing encryption key is required",
    );
    failure(
        artisan(
            root.path(),
            &["env:encrypt", "--readable", "--key", WRONG, "--prune"],
            None,
        ),
        "Unable to decrypt",
    );
    assert_eq!(fs::read_to_string(&encrypted).unwrap(), preserved);
    assert_eq!(fs::read_to_string(&source).unwrap(), edited);

    success(artisan(
        root.path(),
        &["env:encrypt", "--readable", "--key", KEY, "--prune"],
        None,
    ));
    assert!(!source.exists());
    let after = fs::read_to_string(&encrypted).unwrap();
    let updated: Vec<_> = after.lines().collect();
    assert_eq!(updated[0], lines[1]);
    assert_ne!(updated[1], lines[0]);
    assert_eq!(updated[2], lines[2]);
    assert_eq!(updated[3], lines[3]);
    success(artisan(root.path(), &["env:decrypt", "--key", KEY], None));
    assert_eq!(fs::read_to_string(source).unwrap(), edited);
}

#[test]
fn artisan_authenticates_ciphertext_and_never_prunes_after_a_failed_write() {
    let root = TestRoot::new();
    let source = root.path().join(".env");
    let encrypted = root.path().join(".env.encrypted");
    fs::write(&source, "APP_NAME=Probe\n").unwrap();
    fs::create_dir(&encrypted).unwrap();
    let failed = artisan(
        root.path(),
        &["env:encrypt", "--force", "--prune", "--key", KEY],
        None,
    );
    assert_eq!(failed.status.code(), Some(1));
    assert!(source.exists());
    fs::remove_dir(&encrypted).unwrap();
    failure(
        artisan(root.path(), &["env:encrypt", "--quiet", "--prune"], None),
        "Supply --key",
    );
    assert!(source.exists());
    assert!(!encrypted.exists());
    success(artisan(
        root.path(),
        &["env:encrypt", "--cipher", "AES-256-GCM", "--key", KEY],
        None,
    ));
    let payload = fs::read_to_string(&encrypted).unwrap();
    let gcm = Encrypter::new(KEY, Cipher::Aes256Gcm).unwrap();
    assert_eq!(gcm.decrypt(&payload, true).unwrap(), b"APP_NAME=Probe\n");
    failure(
        artisan(root.path(), &["env:encrypt", "--key", KEY], None),
        "Encrypted environment file already exists",
    );
    let tampered = gcm.encrypt_string(b"invalid serialized payload").unwrap();
    fs::write(&encrypted, tampered).unwrap();
    let failed = artisan(
        root.path(),
        &[
            "env:decrypt",
            "--force",
            "--cipher",
            "AES-256-GCM",
            "--key",
            KEY,
        ],
        None,
    );
    assert_eq!(failed.status.code(), Some(1));
    assert_eq!(fs::read_to_string(source).unwrap(), "APP_NAME=Probe\n");
}
