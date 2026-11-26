use openrust::foundation::console::source_edit::UserModelSource;
const SOURCE: &str = r#"
// 中文与伪声明：#[derive(Clone)] impl openrust_passport::HasApiTokens for User
pub(crate) struct User(ModelState);
impl Model for User { fn metadata() -> &'static ModelMetadata {
    static UNUSED: ModelMetadata = ModelMetadata::new("unused").hidden(&["two_factor_secret", "two_factor_recovery_codes"]);
    static META: ModelMetadata = ModelMetadata::new("users")
        .fillable(&["two_factor_secret", "two_factor_recovery_codes"])
        .hidden(&["two_factor_secret", "two_factor_recovery_codes"])
        .hidden(&[User::PASSWORD, User::REMEMBER_TOKEN]);
    &META
}}
"#;
#[test]
fn edits_only_effective_hidden_and_preserves_custom_source() {
    let updated = UserModelSource::parse(SOURCE)
        .unwrap()
        .with_hidden(&["two_factor_secret", "two_factor_recovery_codes"])
        .unwrap();
    assert!(updated.contains(".hidden(&[User::PASSWORD, User::REMEMBER_TOKEN, \"two_factor_secret\", \"two_factor_recovery_codes\",])"));
    let added = ", \"two_factor_secret\", \"two_factor_recovery_codes\",";
    assert_eq!(updated.replacen(added, "", 1), SOURCE);
    assert_eq!(
        UserModelSource::parse(&updated)
            .unwrap()
            .with_hidden(&["two_factor_secret", "two_factor_recovery_codes"])
            .unwrap(),
        updated
    );
}
#[test]
fn comments_strings_other_models_do_not_supply_user_declarations() {
    let source = format!(
        "{SOURCE}\nconst EXAMPLE: &str = r#\"impl openrust_passport::HasApiTokens for User\"#;\nimpl openrust_passport::HasApiTokens for Other {{}}"
    );
    assert!(
        !UserModelSource::parse(&source)
            .unwrap()
            .implements("openrust_passport::HasApiTokens")
    );
    let cloned = UserModelSource::parse(&source)
        .unwrap()
        .with_clone()
        .unwrap();
    assert!(cloned.contains("#[derive(Clone)]\npub(crate) struct User"));
    assert_eq!(
        UserModelSource::parse(&cloned)
            .unwrap()
            .with_clone()
            .unwrap(),
        cloned
    );
    for decl in [
        "#[derive(Debug, Clone)] pub(crate) struct User(ModelState);",
        "pub(crate) struct User(ModelState); impl Clone for User {}",
    ] {
        assert_eq!(
            UserModelSource::parse(decl).unwrap().with_clone().unwrap(),
            decl
        );
    }
    let declared =
        format!("{source}\nimpl openrust_passport::HasApiTokens for User {{ fn marker() {{}} }}");
    assert!(
        UserModelSource::parse(&declared)
            .unwrap()
            .implements("openrust_passport::HasApiTokens")
    );
}
#[test]
fn partial_raw_hidden_and_trailing_comma_are_idempotent() {
    let source = SOURCE.replace(
        ".hidden(&[User::PASSWORD, User::REMEMBER_TOKEN])",
        ".hidden(&[r#\"two_factor_secret\"#,])",
    );
    let updated = UserModelSource::parse(&source)
        .unwrap()
        .with_hidden(&["two_factor_secret", "two_factor_recovery_codes"])
        .unwrap();
    assert!(
        updated.contains(".hidden(&[r#\"two_factor_secret\"#, \"two_factor_recovery_codes\",])")
    );
    assert_eq!(
        UserModelSource::parse(&updated)
            .unwrap()
            .with_hidden(&["two_factor_secret", "two_factor_recovery_codes"])
            .unwrap(),
        updated
    );
}
#[test]
fn unsafe_custom_metadata_and_invalid_syntax_are_rejected() {
    let source = SOURCE.replace(
        ".hidden(&[User::PASSWORD, User::REMEMBER_TOKEN])",
        ".hidden(CUSTOM_HIDDEN)",
    );
    assert!(
        UserModelSource::parse(&source)
            .unwrap()
            .with_hidden(&["two_factor_secret"])
            .is_err()
    );
    assert!(UserModelSource::parse("fn broken( {").is_err());
    assert!(
        UserModelSource::parse("struct User { custom: Custom }")
            .unwrap()
            .with_clone()
            .is_err()
    );
}
