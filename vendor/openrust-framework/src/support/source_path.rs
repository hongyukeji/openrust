/// 从已统一分隔符的 Cargo 缓存路径识别注册表包名或 Git 仓库名。
pub fn cargo_package(path: &str) -> Option<&str> {
    // 自定义 CARGO_HOME 和发布路径重映射可移除 .cargo 目录名。
    if let Some((_, tail)) = path.split_once("/registry/src/")
        && let Some((index, source)) = tail.split_once('/')
        && strip_cargo_cache_fingerprint(index).is_some()
    {
        let (package, _) = source.split_once('/')?;
        // 包名没有点号，语义版本的第一处点号紧跟主版本号。
        let (name_and_major, _) = package.split_once('.')?;
        let (name, major) = name_and_major.rsplit_once('-')?;
        return (!major.is_empty() && major.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(name);
    }
    let (_, tail) = path.split_once("/git/checkouts/")?;
    let (checkout, _) = tail.split_once('/')?;
    strip_cargo_cache_fingerprint(checkout)
}

fn strip_cargo_cache_fingerprint(component: &str) -> Option<&str> {
    let (name, fingerprint) = component.rsplit_once('-')?;
    (fingerprint.len() == 16 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(name)
}

#[cfg(test)]
mod tests {
    use super::cargo_package;

    #[test]
    fn cargo_cache_paths_preserve_the_full_package_or_repository_name() {
        let registry =
            "/build/openrust-framework-cache/registry/src/index.crates.io-1949cf8c6b5b557f";
        for (directory, name) in [
            ("openrust-pulse-0.1.0", "openrust-pulse"),
            (
                "openrust-pulse-exporter-1.2.3-alpha.2",
                "openrust-pulse-exporter",
            ),
            ("observer-0.1.0", "observer"),
        ] {
            let path = format!("{registry}/{directory}/src/lib.rs");
            assert_eq!(cargo_package(&path), Some(name), "{path}");
        }
        let path =
            "/build/cargo/git/checkouts/openrust-framework-3d5e6e0871bfd2bd/1c2f5a8/src/lib.rs";
        assert_eq!(cargo_package(path), Some("openrust-framework"));
        assert_eq!(
            cargo_package("/app/registry/src/notes/module/src/lib.rs"),
            None
        );
    }
}
