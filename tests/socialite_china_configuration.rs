use openrust::foundation::bootstrap::LoadEnvironmentVariables;
use serde_json::json;
use std::{fs, path::PathBuf};
#[path = "../config/mod.rs"]
mod config;
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn tencent_login_configuration_uses_separate_application_credentials() {
    let root = Root(std::env::temp_dir().join(format!(
            "openrust-tencent-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&root.0).unwrap();
    let mut contents = String::new();
    for prefix in ["WEIXIN", "WEIXINWEB", "QQ"] {
        contents.push_str(&format!("{prefix}_CLIENT_ID={prefix}-client\n{prefix}_CLIENT_SECRET={prefix}-secret\n{prefix}_REDIRECT_URI=/{prefix}/callback\n"));
    }
    contents.push_str("WEIXIN_MINI_APP_APPID=mini-app\nWEIXIN_MINI_APP_SECRET=mini-secret\n");
    fs::write(root.0.join(".env"), contents).unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, None).unwrap();
    let configured = config::load(&root.0, &environment).unwrap();
    assert_eq!(
        configured["services"]["weixin_mini_app"],
        json!({"app_id":"mini-app","secret":"mini-secret"})
    );
    for (driver, prefix) in [
        ("weixin", "WEIXIN"),
        ("weixinweb", "WEIXINWEB"),
        ("qq", "QQ"),
    ] {
        assert_eq!(
            configured["services"][driver],
            json!({"client_id":format!("{prefix}-client"),"client_secret":format!("{prefix}-secret"),"redirect":format!("/{prefix}/callback")})
        );
    }
}
