use openrust::foundation::console::source_edit::replace_generated_method;

#[test]
fn generated_action_upgrade_preserves_surrounding_code_and_line_endings() {
    let stock = "impl CreatesUsers for Action { fn create(&self, value: u8) -> u8 { value } }";
    let replacement =
        "impl CreatesUsers for Action { fn create(&self, value: u8) -> u8 { value + 1 } }";
    let source = "// app rule\r\nimpl CreatesUsers for Action { fn create(&self, value: u8) -> u8 { /* generated */ value } fn custom(&self) {} }\r\nfn other() {}\r\n";
    let result =
        replace_generated_method(source, stock, replacement, "CreatesUsers", "create").unwrap();
    assert!(result.contains("// app rule\r\n"));
    assert!(result.contains("value + 1"));
    assert!(result.contains("fn custom(&self) {}"));
    assert!(result.ends_with("fn other() {}\r\n"));
    assert_eq!(
        replace_generated_method(&result, replacement, replacement, "CreatesUsers", "create")
            .unwrap(),
        result
    );
}
#[test]
fn custom_signatures_bodies_and_ambiguous_actions_are_rejected() {
    let stock = "impl CreatesUsers for Action { fn create(&self, value: u8) -> u8 { value } }";
    for source in [
        "impl CreatesUsers for Action { fn create(&self, value: u8) -> u8 { custom(value) } }",
        "impl CreatesUsers for Action { fn create(&mut self, value: u8) -> u8 { value } }",
        "impl CreatesUsers for A { fn create(&self, value: u8) -> u8 { value } } impl CreatesUsers for B { fn create(&self, value: u8) -> u8 { value } }",
        "fn create(value: u8) -> u8 { value }",
        "impl CreatesUsers for Action { #[cfg(feature=\"custom\")] fn create(&self, value: u8) -> u8 { value } }",
        "impl CreatesUsers for Action { async fn create(&self, value: u8) -> u8 { value } }",
    ] {
        assert!(replace_generated_method(source, stock, stock, "CreatesUsers", "create").is_err());
    }
}

#[test]
fn user_token_method_preserves_other_overrides_and_rejects_custom_issuance() {
    use openrust::foundation::console::source_edit::UserModelSource;
    let template =
        "impl openrust::sanctum::HasApiTokens for User { fn create_token(&self) { issued() } }";
    let original = "struct User; impl openrust::sanctum::HasApiTokens for User { fn generate_token_string(&self) { custom() } }";
    let upgraded = UserModelSource::parse(original)
        .unwrap()
        .with_trait_method(template, "openrust::sanctum::HasApiTokens", "create_token")
        .unwrap();
    assert!(upgraded.contains("fn generate_token_string(&self) { custom() }"));
    assert!(upgraded.contains("fn create_token(&self) { issued() }"));
    assert_eq!(
        UserModelSource::parse(&upgraded)
            .unwrap()
            .with_trait_method(template, "openrust::sanctum::HasApiTokens", "create_token")
            .unwrap(),
        upgraded
    );
    for source in [
        "struct User; impl other::HasApiTokens for User {}",
        "struct User; impl openrust::sanctum::HasApiTokens for User {fn create_token(&self) {custom()} }",
    ] {
        assert!(
            UserModelSource::parse(source)
                .unwrap()
                .with_trait_method(template, "openrust::sanctum::HasApiTokens", "create_token")
                .is_err()
        );
    }
}

#[test]
fn ordinary_try_method_spacing_is_equivalent_but_macro_punctuation_is_preserved() {
    let stock = "async fn save() { call()?.next().await?; }";
    let source = "async fn save() { call()?\n .next()\n .await?; }";
    assert_eq!(
        openrust::foundation::console::source_edit::replace_generated_function(
            source, stock, stock, "save"
        )
        .unwrap(),
        source
    );
    let stock = "fn save() { m!(a>>b) }";
    let source = "fn save() { m!(a> >b) }";
    assert!(
        openrust::foundation::console::source_edit::replace_generated_function(
            source, stock, stock, "save"
        )
        .is_err()
    );
}
