//! 包发现只允许宿主能链接的声明进入候选 Rust 清单。

use super::*;
use clap::Parser as _;
use serde_json::json;

#[test]
fn discovery_defaults_to_rebuild_and_modes_are_exclusive() {
    for mode in [None, Some("--install"), Some("--preview"), Some("--check")] {
        let args = ["openrust", "package:discover"].into_iter().chain(mode);
        assert!(crate::Cli::try_parse_from(args).is_ok());
    }
    for modes in [
        ["--install", "--preview"],
        ["--install", "--check"],
        ["--preview", "--check"],
    ] {
        let args = ["openrust", "package:discover"].into_iter().chain(modes);
        assert!(crate::Cli::try_parse_from(args).is_err());
    }
    assert!(
        crate::Cli::try_parse_from(["openrust", "package:list", "--features", "provider"]).is_ok()
    );
    assert!(crate::Cli::try_parse_from(["openrust", "package:list", "--install"]).is_err());
}

#[test]
fn optional_features_respect_alias_activation_and_weak_forwarding() -> Result<()> {
    let host = json!({"name": "app", "dependencies": [{"name": "storage"}], "features": {
        "default": ["weak"],
        "weak": ["storage?/s3"],
        "strong": ["storage/s3"],
        "with-provider": ["indirect"],
        "indirect": ["dep:renamed"],
        "storage": ["dep:storage"]
    }});
    assert!(active_optional_dependencies(&host, &[], false)?.is_empty());
    assert_eq!(
        active_optional_dependencies(
            &host,
            &["app/with-provider".into(), "storage/s3".into()],
            true
        )?,
        BTreeSet::from(["renamed", "storage"])
    );
    assert!(active_optional_dependencies(&host, &["undeclared".into()], false).is_err());
    Ok(())
}

#[test]
fn rejects_workspace_union_and_preserves_the_target_of_the_normal_edge() -> Result<()> {
    let manifest = Path::new("/app/Cargo.toml");
    let package = |id, name, version, lib| {
        json!({"id": id, "name": name, "version": version,
            "manifest_path": "/private/checkout/Cargo.toml",
            "source": "git+https://example.invalid/private",
            "targets": [{"name": lib, "kind": ["lib"]}], "metadata": {"openrust": {"discovery": {
            "version": 1, "providers": true
        }}}})
    };
    let graph = json!({
        "packages": [
            {"id": "host", "name": "app", "manifest_path": manifest,
             "features": {"extra": ["dep:with-extra"]},
             "dependencies": [
                {"name": "real-package", "req": "^0.1", "source": "git+https://example.invalid/private", "rename": null, "kind": null, "target": null, "optional": false},
                {"name": "real-package", "req": "^0.2", "source": "git+https://example.invalid/private", "rename": "with-extra", "kind": null, "target": null, "optional": true},
                {"name": "mixed-package", "req": "*", "source": "git+https://example.invalid/private", "rename": "mixed", "kind": null, "target": "cfg(target_os = \"linux\")", "optional": false},
                {"name": "mixed-package", "rename": "mixed", "kind": "dev", "target": "cfg(target_os = \"macos\")", "optional": false},
                {"name": "build-package", "rename": "build_only", "kind": "build", "target": null, "optional": false}
             ]},
            package("real", "real-package", "0.1.0", "direct"), package("extra", "real-package", "0.2.0", "with_extra"),
            package("mixed", "mixed-package", "0.1.0", "mixed"), package("build", "build-package", "0.1.0", "build_only"),
            package("transitive", "transitive-package", "0.1.0", "transitive")
        ],
        "resolve": {"nodes": [{"id": "host", "features": ["extra"], "deps": [
            {"pkg": "real", "name": "direct", "dep_kinds": [{"kind": null, "target": null}]},
            {"pkg": "extra", "name": "with_extra", "dep_kinds": [{"kind": null, "target": null}]},
            {"pkg": "mixed", "name": "mixed", "dep_kinds": [
                {"kind": null, "target": "cfg(target_os = \"linux\")"},
                {"kind": "dev", "target": "cfg(target_os = \"macos\")"}
            ]},
            {"pkg": "build", "name": "build_only", "dep_kinds": [{"kind": "build", "target": null}]}
        ]}]}
    });
    let sources = Sources::default();
    let source = plan(
        &graph,
        manifest,
        "aarch64-apple-darwin",
        &[],
        false,
        &sources,
    )?
    .source;
    assert!(source.contains("(\"real-package@0.1.0 (direct)\", ::r#direct::openrust_providers)"));
    assert!(
        source.contains(
            "#[cfg(target_os = \"linux\")]\n        (\"mixed-package@0.1.0 (mixed)\", ::r#mixed::openrust_providers)"
        )
    );
    assert!(source.contains("Vec<::openrust::foundation::PackageProvider>"));
    assert!(source.contains("PackageProvider::new(origin, provider)"));
    assert!(!source.contains("/private/checkout"));
    assert!(!source.contains("https://example.invalid/private"));
    for not_linked in ["with_extra", "build_only", "transitive"] {
        assert!(!source.contains(not_linked));
    }
    let selected = plan(
        &graph,
        manifest,
        "aarch64-apple-darwin",
        &["extra".into()],
        true,
        &sources,
    )?
    .source;
    assert!(
        selected
            .contains("(\"real-package@0.2.0 (with_extra)\", ::r#with_extra::openrust_providers)")
    );
    Ok(())
}

fn same_name_sources() -> Value {
    let package = |id, source: &str, lib| {
        json!({"id": id, "name": "twin", "version": "0.1.0",
            "source": format!("{source}#{}", "a".repeat(40)),
            "targets": [{"name": lib, "kind": ["lib"]}],
            "metadata": {"openrust": {"discovery": {"version": 1, "providers": true}}}})
    };
    let a = "git+https://example.invalid/a?branch=stable/v1";
    let b = "git+https://example.invalid/b?tag=v0.1";
    json!({
        "packages": [
            {"id": "host", "name": "app", "manifest_path": "/app/Cargo.toml",
             "features": {"extra": ["dep:renamed-b"]},
             "dependencies": [
                {"name": "twin", "req": "*", "source": a, "rename": null, "kind": null, "optional": false},
                {"name": "twin", "req": "*", "source": b, "rename": "renamed-b", "kind": null, "optional": true}
             ]},
            package("a", a, "a_lib"),
            package("b", b, "renamed_b")
        ],
        "resolve": {"nodes": [{"id": "host", "deps": [
            {"pkg": "a", "name": "a_lib", "dep_kinds": [{"kind": null, "target": null}]},
            {"pkg": "b", "name": "renamed_b", "dep_kinds": [{"kind": null, "target": null}]}
        ]}]}
    })
}

#[test]
fn optional_alias_cannot_borrow_an_unrenamed_dependency_from_another_git_source() -> Result<()> {
    let graph = same_name_sources();
    let manifest = Path::new("/app/Cargo.toml");
    let sources = Sources::default();
    let default = plan(
        &graph,
        manifest,
        "aarch64-apple-darwin",
        &[],
        true,
        &sources,
    )?;
    assert!(default.source.contains("::r#a_lib::openrust_providers"));
    assert!(!default.source.contains("::r#renamed_b::openrust_providers"));
    assert_eq!(default.packages[0]["source"], "git");
    assert_eq!(default.packages[0]["decision"], "selected");
    assert_eq!(default.packages[1]["decision"], "inactive_feature");
    assert!(default.packages[1]["condition"].is_null());
    assert!(!serde_json::to_string(&default.packages)?.contains("example.invalid"));
    let enabled = plan(
        &graph,
        manifest,
        "aarch64-apple-darwin",
        &["extra".into()],
        true,
        &sources,
    )?;
    assert!(enabled.source.contains("::r#a_lib::openrust_providers"));
    assert!(enabled.source.contains("::r#renamed_b::openrust_providers"));
    assert_eq!(enabled.packages[1]["decision"], "selected");
    Ok(())
}

#[test]
fn uncertain_source_is_rejected_without_printing_the_repository() {
    let mut graph = same_name_sources();
    graph["packages"][2]["source"] = json!("git+https://example.invalid/unproven?tag=v0.1");
    let result = plan(
        &graph,
        Path::new("/app/Cargo.toml"),
        "aarch64-apple-darwin",
        &[],
        true,
        &Sources::default(),
    );
    assert!(
        matches!(result, Err(error) if error.to_string().contains("无法确认 Provider 包 `twin` 的 Cargo 来源") && !error.to_string().contains("example.invalid"))
    );
}

#[test]
fn one_declaration_matching_two_resolved_packages_is_rejected() {
    let mut graph = same_name_sources();
    graph["packages"][2]["source"] = graph["packages"][1]["source"].clone();
    graph["packages"][2]["version"] = json!("0.2.0");
    let result = plan(
        &graph,
        Path::new("/app/Cargo.toml"),
        "aarch64-apple-darwin",
        &[],
        true,
        &Sources::default(),
    );
    assert!(matches!(result, Err(error) if error.to_string().contains("Provider 包来源有歧义")));
}

#[test]
fn package_exclusions_and_facade_reexports_follow_declaration_selection() -> Result<()> {
    let first = json!({"version": 1, "providers": true, "aliases": {"Shared": "facades::Old"}});
    let last = json!({"version": 1, "providers": true, "aliases": {"Shared": "New"}});
    let excluded = json!({"version": 1, "dont-discover": ["first"]});
    let declarations = [
        Declaration {
            package: "first",
            version: "1.0.0",
            library: "alias_first",
            options: &first,
            source: "path",
            activation: Activation::Active("all()".into()),
        },
        Declaration {
            package: "last",
            version: "1.0.0",
            library: "alias_last",
            options: &last,
            source: "registry",
            activation: Activation::Active("unix".into()),
        },
        Declaration {
            package: "excluded",
            version: "1.0.0",
            library: "exclusion",
            options: &excluded,
            source: "git",
            activation: Activation::Active("windows".into()),
        },
    ];
    let host = json!({"name": "app", "metadata": {"openrust": {"discovery": {
        "version": 1, "dont-discover": ["excluded"]
    }}}});
    let rendered = render(&host, &declarations)?;
    assert_eq!(rendered.packages[0]["decision"], "conditional_exclusion");
    assert_eq!(
        rendered.packages[0]["excluded_by"][0]["package"],
        "excluded"
    );
    assert_eq!(rendered.packages[2]["decision"], "excluded_by_application");
    assert_eq!(rendered.aliases[0]["name"], "Shared");
    assert_eq!(
        rendered.aliases[0]["candidates"][0]["condition"],
        "all(all(all(), not(windows)), not(unix))"
    );
    assert_eq!(
        rendered.aliases[0]["candidates"][1]["path"],
        "::r#alias_last::r#New"
    );
    let source = rendered.source;
    assert!(source.contains("all(all(), not(windows))"));
    assert!(source.contains("not(unix)"));
    assert!(source.contains("::r#alias_first::r#facades::r#Old as r#Shared"));
    assert!(source.contains("::r#alias_last::r#New as r#Shared"));
    let host = json!({"name": "app", "metadata": {"openrust": {"discovery": {
        "version": 1, "dont-discover": ["*"]
    }}}});
    let rendered = render(&host, &[])?;
    assert!(rendered.global_opt_out);
    assert_eq!(rendered.packages, [] as [Value; 0]);
    assert!(!rendered.source.contains("::r#alias_"));
    Ok(())
}

#[test]
fn rejects_non_paths_in_facade_metadata() {
    let declaration = json!({"version": 1, "aliases": {"Alias": "path; fn fake() {}"}});
    assert!(collect_aliases(&declaration, "r#valid", "all()", &mut BTreeMap::new()).is_err());
}

#[test]
fn refuses_rust_only_build_conditions_for_cargo_dependency_edges() -> Result<()> {
    assert!(target_condition("cfg(any(test, feature = \"tools\"))").is_err());
    assert_eq!(
        target_condition("cfg(target_feature = \"test\")")?,
        Some("target_feature = \"test\"")
    );
    Ok(())
}
