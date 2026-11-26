use openrust_blade_compiler::blocks;

#[test]
fn rust_literals_comments_and_lifetimes_do_not_close_a_block() {
    let source = r####"@rust
        let text = r###"@endrust {{-- Blade --}} @verbatim"###;
        let bytes = br"@endrust";
        let c = cr#"@endrust"#;
        let character = '\'';
        fn borrow<'a>(s: &'a str) -> &'a str { s }
        // @endrust
        /* @endrust /* nested @endrust */ */
        let value = "@endrust";
        macro_rules! hidden { (@endrust) => { 42 }; }
        let _ = hidden!(@endrust);
    @endrust{{ $value }}@rust let x = 2; @endrust"####;
    let found = blocks(source).unwrap();
    assert_eq!(found.len(), 2);
    assert!(found[0].body.contains("fn borrow<'a>"));
    assert_eq!(found[1].body, " let x = 2; ");
}

#[test]
fn blade_literals_comments_and_expressions_are_not_executable() {
    let source = r#"{{-- @rust invalid @endrust --}}@verbatim @rust ignored @endrust @endverbatim @@rust @@endrust {{ '@rust' }} {!! '@endrust' !!} @include('has @rust in name') @rust let x=1; @endrust"#;
    assert_eq!(blocks(source).unwrap().len(), 1);
}

#[test]
fn unmatched_boundaries_fail_with_line_information() {
    assert!(
        blocks("first\n@rust let x = 3;")
            .unwrap_err()
            .to_string()
            .contains('2')
    );
    assert!(blocks("@endrust").is_err());
    assert!(blocks("@rust let s = r#\"@endrust\";").is_err());
    assert!(blocks("@rust /* @endrust").is_err());
}
