//! 包发现只允许宿主能链接的声明进入候选 Rust 清单。

use super::*;
use serde_json::json;

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
    let package = |id, name| {
        json!({"id": id, "name": name, "targets": [{"name": "direct", "kind": ["lib"]}], "metadata": {"openrust": {"discovery": {
            "version": 1, "providers": true
        }}}})
    };
    let graph = json!({
        "packages": [
            {"id": "host", "name": "app", "manifest_path": manifest,
             "features": {"extra": ["dep:with-extra"]},
             "dependencies": [
                {"name": "real-package", "rename": null, "kind": null, "target": null, "optional": false},
                {"name": "real-package", "rename": "with-extra", "kind": null, "target": null, "optional": true},
                {"name": "mixed-package", "rename": "mixed", "kind": null, "target": "cfg(target_os = \"linux\")", "optional": false},
                {"name": "mixed-package", "rename": "mixed", "kind": "dev", "target": "cfg(target_os = \"macos\")", "optional": false},
                {"name": "build-package", "rename": "build_only", "kind": "build", "target": null, "optional": false}
             ]},
            package("real", "real-package"), package("extra", "real-package"),
            package("mixed", "mixed-package"), package("build", "build-package"),
            package("transitive", "transitive-package")
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
    let source = plan(&graph, manifest, "aarch64-apple-darwin", &[], false)?;
    assert!(source.contains("::r#direct::openrust_providers"));
    assert!(
        source.contains("#[cfg(target_os = \"linux\")]\n        ::r#mixed::openrust_providers")
    );
    for not_linked in ["with_extra", "build_only", "transitive"] {
        assert!(!source.contains(not_linked));
    }
    let selected = plan(
        &graph,
        manifest,
        "aarch64-apple-darwin",
        &["extra".into()],
        true,
    )?;
    assert!(selected.contains("::r#with_extra::openrust_providers"));
    Ok(())
}

#[test]
fn package_exclusions_and_facade_reexports_follow_declaration_selection() -> Result<()> {
    let first = json!({"version": 1, "providers": true, "aliases": {"Shared": "facades::Old"}});
    let last = json!({"version": 1, "providers": true, "aliases": {"Shared": "New"}});
    let excluded = json!({"version": 1, "dont-discover": ["first"]});
    let declarations = [
        Declaration {
            package: "first",
            library: "alias_first",
            options: &first,
            active: "all()".into(),
        },
        Declaration {
            package: "last",
            library: "alias_last",
            options: &last,
            active: "unix".into(),
        },
        Declaration {
            package: "excluded",
            library: "exclusion",
            options: &excluded,
            active: "windows".into(),
        },
    ];
    let host = json!({"name": "app", "metadata": {"openrust": {"discovery": {
        "version": 1, "dont-discover": ["excluded"]
    }}}});
    let source = render(&host, &declarations)?;
    assert!(source.contains("all(all(), not(windows))"));
    assert!(source.contains("not(unix)"));
    assert!(source.contains("::r#alias_first::r#facades::r#Old as r#Shared"));
    assert!(source.contains("::r#alias_last::r#New as r#Shared"));
    let host = json!({"name": "app", "metadata": {"openrust": {"discovery": {
        "version": 1, "dont-discover": ["*"]
    }}}});
    let source = render(&host, &declarations)?;
    assert!(!source.contains("::r#alias_"));
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
