use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::{
    app,
    env::{get, path},
};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app: &app::Loaded<'_>,
) -> Result<Value, ConfigError> {
    let optional_aws = |key| {
        get(env, key, Value::Null).map(|value| {
            if value.as_str().is_some_and(str::is_empty) {
                Value::Null
            } else {
                value
            }
        })
    };
    let default_url = format!("{}/storage", app.url.trim_end_matches('/'));
    let local_root = match env.get_utf8("FILESYSTEM_LOCAL_ROOT")? {
        Some(path) => base_path.join(path),
        None => env.storage_path(base_path, "app/private"),
    };
    let public_root = match env.get_utf8("FILESYSTEM_PUBLIC_ROOT")? {
        Some(path) => base_path.join(path),
        None => env.storage_path(base_path, "app/public"),
    };
    let web_root = env
        .get_utf8("FILESYSTEM_WEB_ROOT")?
        .unwrap_or(app.public_path);
    if web_root.trim().is_empty() {
        return Err(ConfigError::InvalidType {
            key: "FILESYSTEM_WEB_ROOT".into(),
            expected: "a non-empty directory path",
        });
    }
    let public_path = base_path.join(web_root);
    let link = public_path.join("storage");
    let target = &public_root;
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认存储磁盘
        |--------------------------------------------------------------------------
        |
        | FILESYSTEM_DISK 决定未指定磁盘时使用哪个存储后端。
        |
        */
        "default": get(env, "FILESYSTEM_DISK", "local")?,

        /*
        |--------------------------------------------------------------------------
        | 本地与远程磁盘
        |--------------------------------------------------------------------------
        |
        | web 用于网站静态文件，local 用于私有文件，public 用于公开存储文件。
        | FILESYSTEM_WEB_ROOT 可覆盖网站目录，未设置时使用 APP_PUBLIC_PATH。
        | FILESYSTEM_LOCAL_ROOT 和 FILESYSTEM_PUBLIC_ROOT 可覆盖对应存储根目录。
        | 相对路径从应用根目录解析；存储根目录未设置时使用存储目录下的默认路径。
        | s3 用于 S3 兼容对象存储。
        |
        */
        "disks": {
            "web": {
                "driver": "local",
                "root": path(&public_path)?,
                "url": env.get_utf8("FILESYSTEM_WEB_URL")?.unwrap_or(&app.url),
                "visibility": "public",
                "throw": false,
                "report": false,
            },
            "local": {
                "driver": "local",
                "root": path(&local_root)?,
                "serve": true,
                "throw": false,
                "report": false,
            },
            "public": {
                "driver": "local",
                "root": path(&public_root)?,
                "url": env.get_utf8("FILESYSTEM_PUBLIC_URL")?.unwrap_or(&default_url),
                "visibility": "public",
                "throw": false,
                "report": false,
            },
            "s3": {
                "driver": "s3",
                "key": optional_aws("AWS_ACCESS_KEY_ID")?,
                "secret": optional_aws("AWS_SECRET_ACCESS_KEY")?,
                "token": optional_aws("AWS_SESSION_TOKEN")?,
                "region": get(env, "AWS_DEFAULT_REGION", "us-east-1")?,
                "bucket": optional_aws("AWS_BUCKET")?,
                "url": optional_aws("AWS_URL")?,
                "endpoint": optional_aws("AWS_ENDPOINT")?,
                "temporary_url": optional_aws("AWS_TEMPORARY_URL")?,
                "use_path_style_endpoint": get(env, "AWS_USE_PATH_STYLE_ENDPOINT", false)?,
                "throw": false,
                "report": false,
            },
        },

        /*
        |--------------------------------------------------------------------------
        | 公共存储链接
        |--------------------------------------------------------------------------
        |
        | artisan storage:link 根据此映射将公共目录下的 storage 指向 public 磁盘根目录。
        |
        */
        "links": {(path(&link)?): path(target)?},
    }))
}
