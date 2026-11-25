//! 套件编译内置的缺键默认值；保留应用语言文件和既有运行时覆盖。
use openrust::{
    foundation::Application,
    localization::{Lines, Localizer},
    support::ProviderResult,
};

pub(super) async fn register(app: &Application) -> ProviderResult {
    let localizer = app.make::<Localizer>().await?;
    for (locale, source) in [
        ("en", include_str!("../../stubs/starter/lang/en.json")),
        (
            "zh-Hans",
            include_str!("../../stubs/starter/lang/zh-Hans.json"),
        ),
        (
            "zh-Hant",
            include_str!("../../stubs/starter/lang/zh-Hant.json"),
        ),
    ] {
        let defaults: Lines = serde_json::from_str(source)?;
        let mut missing = Vec::new();
        for (key, line) in defaults {
            let key = format!("user.{key}");
            if !localizer.has_for_locale(&key, Some(locale))? {
                missing.push((key, line));
            }
        }
        localizer.add_lines(missing, locale, None)?;
    }
    Ok(())
}
