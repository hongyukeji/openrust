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
    let default_url = format!("{}/storage", app.url.trim_end_matches('/'));
    let public_root = env
        .get_utf8("PUBLIC_STORAGE_ROOT")?
        .unwrap_or("storage/app/public");
    let link = base_path
        .join(app.public_path.unwrap_or("public"))
        .join("storage");
    let target = base_path.join(public_root);
    Ok(json!({
        "default": get(env, "FILESYSTEM_DISK", "local")?,
        "disks": {
            "local": {
                "driver": "local",
                "root": path(&base_path.join("storage/app/private"))?,
                "serve": true,
                "throw": false,
                "report": false,
            },
            "public": {
                "driver": "local",
                "root": public_root,
                "url": env.get_utf8("PUBLIC_STORAGE_URL")?.unwrap_or(&default_url),
                "visibility": "public",
                "throw": false,
                "report": false,
            },
            "s3": {
                "driver": "s3",
                "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
                "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
                "region": get(env, "AWS_DEFAULT_REGION", Value::Null)?,
                "bucket": get(env, "AWS_BUCKET", Value::Null)?,
                "url": get(env, "AWS_URL", Value::Null)?,
                "endpoint": get(env, "AWS_ENDPOINT", Value::Null)?,
                "use_path_style_endpoint": get(env, "AWS_USE_PATH_STYLE_ENDPOINT", false)?,
                "throw": false,
                "report": false,
            },
        },
        "links": {(path(&link)?): path(&target)?},
    }))
}
