use openrust::foundation::PackageManifest;

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/packages.rs"));
}

// 别名按需使用，未引用时不产生警告。
#[allow(unused_imports)]
pub(crate) use compiled::package_aliases;

/// 返回当前应用编译时自动发现的包清单。
pub(super) const fn manifest() -> PackageManifest {
    compiled::PACKAGES
}
