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
fn artisan_text_lists_show_localized_global_options() {
    for (locale, heading, description) in [
        (
            "en",
            "Options:",
            "The environment the command should run under",
        ),
        ("zh-Hans", "选项：", "指定运行环境"),
        ("zh-Hant", "選項：", "指定執行環境"),
    ] {
        for args in [
            vec!["list", "--no-ansi"],
            vec!["list", "cache", "--no-ansi"],
        ] {
            let output = artisan_in(&args, locale, None);
            assert!(output.status.success());
            let listing = String::from_utf8(output.stdout).unwrap();
            let (_, options) = listing.split_once(heading).unwrap();
            let (options, _) = options.split_once("\n\n").unwrap();
            for option in [
                "--env",
                "--no-interaction",
                "--ansi",
                "--silent",
                "--help",
                "--version",
            ] {
                assert!(
                    options.contains(option),
                    "{option} is missing from {options}"
                );
            }
            assert!(options.contains(description));
            assert!(!options.contains("--format"));
        }
        let raw = artisan_in(&["list", "cache", "--raw", "--ansi"], locale, None);
        assert!(raw.status.success());
        assert!(!raw.stdout.contains(&0x1b));
        let raw = String::from_utf8(raw.stdout).unwrap();
        assert!(!raw.contains(heading) && !raw.contains("--env"));
    }
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
fn artisan_reload_excludes_tasks_by_label_and_command() {
    let help = artisan(&["help", "reload"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--except"));

    let result = artisan(&[
        "reload",
        "--except",
        " queue, schedule:interrupt, reverb, queue, ",
    ]);
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("Reloading services."));
    assert!(!output.contains("DONE"));
}

#[test]
fn artisan_optimization_commands_allow_excluding_tasks() {
    for (command, skip) in [
        (
            "optimize",
            "config, event:cache, routes, views, config:cache",
        ),
        (
            "optimize:clear",
            "config, cache:clear, compiled, events, route:clear, views",
        ),
    ] {
        let help = artisan(&["help", command]);
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("--except"));

        let result = artisan(&[command, "-e", skip]);
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&result.stdout).contains("DONE"));
    }
}

#[test]
fn artisan_raw_help_keeps_full_localized_text_without_ansi() {
    for (locale, usage, options) in [
        ("en", "Usage:", "Options:"),
        ("zh-Hans", "用法：", "选项："),
        ("zh-Hant", "用法：", "選項："),
    ] {
        let plain = artisan_in(&["help", "list", "--no-ansi"], locale, None);
        let raw = artisan_in(&["help", "list", "--raw", "--ansi"], locale, None);
        let decorated = artisan_in(&["help", "list", "--ansi"], locale, None);
        let direct = artisan_in(&["list", "--help", "--ansi"], locale, None);
        for output in [&plain, &raw, &decorated, &direct] {
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
        }
        assert_eq!(raw.stdout, plain.stdout);
        assert_eq!(decorated.stdout, direct.stdout);
        assert!(!raw.stdout.contains(&0x1b));
        assert!(decorated.stdout.contains(&0x1b));
        let decorated = String::from_utf8(decorated.stdout).unwrap();
        assert!(decorated.contains(usage) && decorated.contains(options));
        assert!(decorated.contains("--format"));
    }
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
    let entries: Vec<_> = list
        .lines()
        .filter(|line| line.split_whitespace().next() == Some("build"))
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].ends_with("[Planned]"));

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
        ("zh-Hans", "选项：", "无法识别的参数"),
        ("zh-Hant", "選項：", "無法辨識的引數"),
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
fn artisan_localizes_production_cancellation_without_a_terminal() {
    for (locale, cancellation) in [
        ("en", "Command cancelled."),
        ("zh-Hans", "命令已取消。"),
        ("zh-Hant", "命令已取消。"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "migrate", "--no-interaction"])
            .env("APP_ENV", "production")
            .env("APP_LOCALE", locale)
            .env("APP_PORT", "invalid")
            .env("DB_CONNECTION", "mysql")
            .env("DB_HOST", "127.0.0.1")
            .env("DB_PORT", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), cancellation);
        assert!(output.stderr.is_empty());
    }
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
    let output = artisan_in(&["help", "list"], "sv", Some(&root));
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Options de secours :") && help.contains("Liste de secours"));
}

#[test]
fn artisan_publishes_consumable_catalogs_without_replacing_bcp47_resources() {
    use openrust::localization::{FileLoader, Localizer};
    use std::{collections::BTreeMap, fs};

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    for relative in ["lang", "resources/lang"] {
        let root = std::env::temp_dir().join(format!(
            "openrust-publish-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let _cleanup = Cleanup(root.clone());
        let catalogs = root.join(relative);
        fs::create_dir_all(catalogs.join("zh-Hans")).unwrap();
        let custom = catalogs.join("zh-Hans/validation.json");
        let contents = r#"{"required":"请填写:attribute。"}"#;
        fs::write(&custom, contents).unwrap();
        let output = artisan_in(&["lang:publish"], "zh-Hans", Some(&root));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(&custom).unwrap(), contents);
        let result = artisan_in(&["lang:publish"], "zh-Hans", Some(&root));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        for name in ["auth", "pagination", "passwords", "validation", "response"] {
            let published = catalogs.join(format!("en/{name}.json"));
            let _: serde_json::Value =
                serde_json::from_slice(&fs::read(published).unwrap()).unwrap();
        }
        assert_eq!(fs::read_to_string(&custom).unwrap(), contents);
        assert!(!catalogs.join("zh_CN").exists());
        if relative == "resources/lang" {
            assert!(!root.join("lang").exists());
        }
        let localizer =
            Localizer::new(FileLoader::new(catalogs), "zh-Hans".into(), "en".into()).unwrap();
        let replacements = BTreeMap::from([("attribute".into(), "email".into())]);
        assert_eq!(
            localizer
                .string("validation.required", &replacements, None)
                .unwrap(),
            "请填写email。"
        );
        assert_eq!(
            localizer
                .string("validation.required", &replacements, Some("zh-CN"))
                .unwrap(),
            "The email field is required."
        );
        assert!(
            localizer
                .string("validation.required", &replacements, Some("zh_CN"))
                .is_err()
        );
    }
}
