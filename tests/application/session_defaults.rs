//! 验证根应用与生成模板共用Session配置键。

#[test]
fn application_and_generated_session_defaults_share_environment_keys() {
    let application = include_str!("../../config/session.rs");
    let generated = include_str!("../../tools/cli/templates/config/session.rs");
    // 根应用在启动时选择profile；生成应用在安装时选择Provider，源码不必相同。
    for defaults in [application, generated] {
        assert!(
            defaults.contains("connection: ctx.nullable_string_or(\"SESSION_CONNECTION\", None)")
        );
        assert!(defaults.contains("table: ctx.string_or(\"SESSION_TABLE\", \"sessions\")?"));
        assert!(defaults.contains("lifetime: 120"));
        assert!(defaults.contains("Some(\"database\".to_owned())"));
    }
}
