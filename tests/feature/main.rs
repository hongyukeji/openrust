use std::process::{Command, Output};

fn artisan(arguments: &[&str]) -> Output {
    artisan_in(arguments, "en", None)
}

fn artisan_in(arguments: &[&str], locale: &str, base: Option<&std::path::Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
    if let Some(base) = base {
        command.env("APP_BASE_PATH", base);
    }
    command
        .arg("artisan")
        .env("APP_LOCALE", locale)
        .args(arguments)
        .env("APP_PORT", "invalid")
        .env_remove("FORCE_COLOR")
        .env_remove("NO_COLOR")
        .output()
        .expect("run Artisan")
}

#[test]
fn artisan_lists_registered_commands_without_starting_http() {
    let default = artisan(&[]);
    let explicit = artisan(&["list"]);
    assert!(default.status.success());
    assert!(explicit.status.success());
    assert_eq!(default.stdout, explicit.stdout);

    let listing = String::from_utf8(explicit.stdout).unwrap();
    assert!(listing.contains("Available commands:"));
    assert!(
        listing
            .lines()
            .any(|line| line.trim_start().starts_with("list "))
    );
    assert!(
        listing
            .lines()
            .any(|line| line.trim_start().starts_with("help "))
    );
    assert!(listing.lines().any(|line| line == " make"));

    let raw = artisan(&["list", "--raw"]);
    assert!(raw.status.success());
    assert!(
        !String::from_utf8(raw.stdout)
            .unwrap()
            .contains("Available commands:")
    );
}

#[test]
fn artisan_supports_both_command_help_forms() {
    for arguments in [["help", "list"], ["list", "--help"]] {
        let help = artisan(&arguments);
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        assert!(
            help.lines()
                .any(|line| line == "Usage: artisan list [OPTIONS] [namespace]")
        );
        assert!(help.contains("--raw"));
    }

    let unknown = artisan(&["help", "unknown"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(!unknown.stderr.is_empty());
}

#[test]
fn artisan_inspire_prints_an_attributed_quote() {
    let output = artisan(&["inspire"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    let output = String::from_utf8(output.stdout).unwrap();
    let mut lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let quote = lines.next().unwrap();
    assert!(quote.starts_with('“') && quote.ends_with('”'));
    assert!(lines.next().unwrap().starts_with("— "));
    assert!(lines.next().is_none());

    let help = artisan(&["help", "inspire"]);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("Display an inspiring quote")
    );
}

#[test]
fn artisan_inspire_honors_ansi_output_flags() {
    let styled = artisan(&["inspire", "--ansi"]);
    assert!(styled.status.success());
    let styled = String::from_utf8(styled.stdout).unwrap();
    assert!(styled.contains("\x1b[1m“ "));
    assert!(styled.contains(" ”\x1b[22m"));
    assert!(styled.contains("\x1b[90m— "));
    assert!(styled.ends_with("\x1b[39m\x1b[33m\n\x1b[39m\n"));

    let plain = artisan(&["--no-ansi", "inspire"]);
    assert!(plain.status.success());
    assert!(!plain.stdout.contains(&0x1b));
    assert!(plain.stdout.ends_with(b"\n\n"));
}

#[test]
fn artisan_reserves_reference_commands_without_running_them() {
    let list = artisan(&["list", "--raw"]);
    assert!(list.status.success());
    let list = String::from_utf8(list.stdout).unwrap();
    for name in ["route:cache", "build"] {
        let entries: Vec<_> = list
            .lines()
            .filter(|line| line.split_whitespace().next() == Some(name))
            .collect();
        assert_eq!(entries.len(), 1, "{name}");
        assert!(entries[0].ends_with("[Planned]"));
    }

    let help = artisan(&["help", "build"]);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("not implemented yet")
    );

    for arguments in [
        ["queue:restart", "--help"],
        ["help", "queue:continue"],
        ["queue:pause", "--help"],
    ] {
        let help = artisan(&arguments);
        assert!(help.status.success(), "{arguments:?}");
        assert!(
            !String::from_utf8(help.stdout)
                .unwrap()
                .contains("not implemented yet")
        );
    }
}

#[test]
fn artisan_lists_descriptions_and_filters_all_output_formats() {
    let output = artisan(&["list", "cache", "--raw"]);
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.lines().all(|line| line.starts_with("cache:")));
    assert!(output.contains("Flush the application cache"));
    assert!(output.contains("cache:table"));

    let output = artisan(&["list", "cache", "--format=json", "--short"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let commands = json["commands"].as_array().unwrap();
    assert_eq!(commands[0]["name"], "cache:clear");
    assert_eq!(commands[0]["implemented"], true);
    assert!(commands[0].get("usage").is_none());

    let output = artisan(&["list", "cache", "--format=xml"]);
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.starts_with("<commands>"));
    assert!(output.contains("<usage>"));

    let output = artisan(&["list", "cache", "--format=md"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("## cache")
    );

    assert_eq!(
        artisan(&["list", "missing-namespace"]).status.code(),
        Some(1)
    );
}

#[test]
fn artisan_invalid_option_uses_the_console_error_status() {
    let output = artisan(&["list", "--unknown-option"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--unknown-option")
    );
}

#[test]
fn artisan_localizes_help_and_errors_without_changing_command_identifiers() {
    for (locale, options, invalid) in [
        ("zh_Hans", "选项：", "无法识别的参数"),
        ("zh_Hant", "選項：", "無法辨識的引數"),
    ] {
        let help = artisan_in(&["help", "list"], locale, None);
        let flag = artisan_in(&["list", "--help"], locale, None);
        assert_eq!(help.stdout, flag.stdout);
        assert!(help.status.success() && flag.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        assert!(
            help.contains(options) && help.contains("--format") && help.contains("artisan list")
        );
        let failure = artisan_in(&["inspire", "--wrong"], locale, None);
        assert_eq!(failure.status.code(), Some(1));
        assert!(failure.stdout.is_empty());
        assert!(String::from_utf8_lossy(&failure.stderr).contains(invalid));
    }
    let abbreviated = artisan(&["help", "auth:clear-r", "--format=json"]);
    assert!(abbreviated.status.success());
    let body: serde_json::Value = serde_json::from_slice(&abbreviated.stdout).unwrap();
    assert_eq!(body["name"], "auth:clear-resets");
    assert_eq!(
        body["definition"]["arguments"]["name"]["is_required"],
        false
    );
    for format in ["xml", "md"] {
        let output = artisan(&["help", "list", "--format", format]);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("artisan list"));
    }
    let quiet = artisan(&["inspire", "-q"]);
    assert!(quiet.status.success() && quiet.stdout.is_empty());
    let silent = artisan(&["help", "missing", "--silent"]);
    assert_eq!(silent.status.code(), Some(1));
    assert!(silent.stdout.is_empty() && silent.stderr.is_empty());
    assert!(artisan(&["--version"]).status.success());
}

#[test]
fn artisan_uses_application_translation_fallback() {
    let root = std::env::temp_dir().join(format!("openrust-artisan-locale-{}", std::process::id()));
    std::fs::create_dir_all(root.join("lang/fr")).unwrap();
    std::fs::write(root.join(".env"), "APP_FALLBACK_LOCALE=fr\n").unwrap();
    std::fs::write(
        root.join("lang/fr/console.json"),
        r#"{"options":"Options de secours :","commands":{"list":{"description":"Liste de secours"}}}"#,
    ).unwrap();
    let output = artisan_in(&["help", "list"], "de", Some(&root));
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Options de secours :") && help.contains("Liste de secours"));
}
