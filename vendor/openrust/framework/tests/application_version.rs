use openrust::foundation::Application;

#[test]
fn repository_comes_from_the_framework_package_without_a_trailing_slash() {
    assert_eq!(Application::REPOSITORY, env!("CARGO_PKG_REPOSITORY"));
    assert!(Application::REPOSITORY.starts_with("https://"));
    assert!(!Application::REPOSITORY.ends_with('/'));
}
