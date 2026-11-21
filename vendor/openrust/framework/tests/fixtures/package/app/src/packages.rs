use openrust::foundation::PackageManifest;

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/packages.rs"));
}

#[allow(unused_imports)]
pub(super) use compiled::package_aliases;

pub(super) const fn manifest() -> PackageManifest {
    compiled::PACKAGES
}

#[cfg(all(test, feature = "package"))]
mod tests {
    #[test]
    fn discovered_alias_and_manifest_refer_to_the_same_package() {
        fn provider<T: openrust::support::ServiceProvider>() {}
        provider::<super::package_aliases::OpenRustProbe>();
        let packages = super::manifest().packages();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "package-probe");
        assert_eq!(packages[0].aliases, &[("OpenRustProbe", "ProbeProvider")]);
    }
}
