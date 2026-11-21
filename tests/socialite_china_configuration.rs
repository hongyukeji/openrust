use openrust::foundation::bootstrap::LoadEnvironmentVariables;
use serde_json::json;
use std::{fs, path::PathBuf};
#[path = "support/config_app.rs"]
mod app;

#[path = "../config/mod.rs"]
mod config;
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "openrust-tencent-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn tencent_login_configuration_groups_wechat_and_keeps_application_credentials_separate() {
    let root = Root::new();
    fs::create_dir(&root.0).unwrap();
    let mut contents = String::new();
    for prefix in ["WECHAT_WEB", "WECHAT_OFFICIAL_ACCOUNT", "QQ"] {
        contents.push_str(&format!("{prefix}_CLIENT_ID={prefix}-client\n{prefix}_CLIENT_SECRET={prefix}-secret\n{prefix}_REDIRECT_URI=/{prefix}/callback\n"));
    }
    contents.push_str("WECHAT_MINI_APP_APPID=mini-app\nWECHAT_MINI_APP_SECRET=mini-secret\n");
    fs::write(root.0.join(".env"), contents).unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, None).unwrap();
    let configured = config::load(&root.0, &environment).unwrap();
    assert_eq!(
        configured["services"]["wechat"]["mini_app"],
        json!({"app_id":"mini-app","secret":"mini-secret"})
    );
    for (entry, prefix) in [
        ("web", "WECHAT_WEB"),
        ("official_account", "WECHAT_OFFICIAL_ACCOUNT"),
    ] {
        assert_eq!(
            configured["services"]["wechat"][entry],
            json!({"client_id":format!("{prefix}-client"),"client_secret":format!("{prefix}-secret"),"redirect":format!("/{prefix}/callback")})
        );
    }
    assert_eq!(
        configured["services"]["qq"],
        json!({"client_id":"QQ-client","client_secret":"QQ-secret","redirect":"/QQ/callback"})
    );
    for old in ["weixin", "weixinweb", "weixin_mini_app"] {
        assert!(configured["services"].get(old).is_none());
    }
    assert_eq!(
        configured["services"]["wechat"].as_object().unwrap().len(),
        3
    );
}
