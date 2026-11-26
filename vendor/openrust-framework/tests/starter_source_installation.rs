use openrust::foundation::console::source_edit::enable_stateful_api;

#[test]
fn stateful_api_edit_preserves_custom_middleware_and_is_idempotent() {
    let source = "fn build() { Application::configure(base).with_middleware(|_middleware| {\n    _middleware.alias(\"custom\", value);\n}).create() }\n";
    let updated = enable_stateful_api(source).unwrap();
    assert!(updated.contains("#[cfg(feature = \"sanctum\")]"));
    assert!(updated.contains("_middleware.stateful_api();"));
    assert!(updated.contains("_middleware.alias(\"custom\", value);"));
    syn::parse_file(&updated).unwrap();
    assert_eq!(enable_stateful_api(&updated).unwrap(), updated);
    let crlf = source.replace('\n', "\r\n");
    let updated = enable_stateful_api(&crlf).unwrap();
    assert!(!updated.replace("\r\n", "").contains('\n'));
}
#[test]
fn manual_callback_and_ambiguous_middleware_are_rejected_without_source_changes() {
    for source in [
        "fn build() { builder.with_middleware(custom).create() }",
        "fn build() { builder.with_middleware(|a|{}).with_middleware(|b|{}).create() }",
        "fn build() {}",
    ] {
        assert!(enable_stateful_api(source).is_err());
    }
}

#[test]
fn generated_function_edits_preserve_extra_business_routes_and_reject_custom_logic() {
    use openrust::foundation::console::source_edit::replace_generated_function;
    let expected = "fn user(request: &mut Request) -> Response { old(request) }";
    let replacement = "fn user(request: &mut Request) -> Response { safe_resource(request) }";
    let source = "// 业务备注\nfn routes() { custom(); }\nfn user( request: &mut Request )-> Response { /*默认实现*/ old(request) }\n";
    let updated = replace_generated_function(source, expected, replacement, "user").unwrap();
    assert!(updated.starts_with("// 业务备注\nfn routes() { custom(); }\n"));
    assert!(updated.contains("safe_resource(request)"));
    syn::parse_file(&updated).unwrap();
    assert!(
        replace_generated_function(
            "fn user(request: &mut Request) -> Response { custom(request) }",
            expected,
            replacement,
            "user"
        )
        .is_err()
    );
    assert!(
        replace_generated_function(
            "fn user(other: &mut Request) -> Response { old(other) }",
            expected,
            replacement,
            "user"
        )
        .is_err()
    );
}
