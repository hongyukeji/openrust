use super::{get, tzif};

#[test]
fn all_fixed_iana_names_load_with_jiff_and_keep_their_identity() {
    assert_eq!(tzif::ZONES.len(), 598);
    for (name, _) in tzif::ZONES {
        let (canonical, bytes) = get(name).unwrap();
        assert_eq!(canonical, *name);
        assert_eq!(get(&name.to_ascii_lowercase()).unwrap().0, *name);
        let timezone = jiff::tz::TimeZone::tzif(canonical, bytes).unwrap();
        assert_eq!(timezone.iana_name(), Some(*name));
    }
    assert!(get("invalid/timezone").is_none());
}
