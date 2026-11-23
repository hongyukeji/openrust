use std::collections::HashSet;

use openrust::database::eloquent::factories::fake;

#[test]
fn faker_blood_samples_cover_the_same_values_across_locales() {
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let types: HashSet<_> = (0..4).map(|_| unique.blood_type().unwrap()).collect();
        assert_eq!(types, HashSet::from(["A", "AB", "B", "O"]));
        assert!(unique.blood_type().is_err());

        let factors: HashSet<_> = (0..2).map(|_| unique.blood_rh().unwrap()).collect();
        assert_eq!(factors, HashSet::from(["+", "-"]));
        assert!(unique.blood_rh().is_err());

        let groups: HashSet<_> = (0..8).map(|_| unique.blood_group().unwrap()).collect();
        let expected = types
            .iter()
            .flat_map(|abo| factors.iter().map(move |rh| format!("{abo}{rh}")))
            .collect();
        assert_eq!(groups, expected);
        assert!(unique.blood_group().is_err());

        let reset = faker.unique(true);
        assert!(types.contains(reset.blood_type().unwrap()));
        assert!(factors.contains(reset.blood_rh().unwrap()));
        assert!(groups.contains(&reset.blood_group().unwrap()));
    }
}
