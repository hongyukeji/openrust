use std::collections::HashSet;

use openrust::database::eloquent::factories::fake;

#[test]
fn faker_blood_samples_cover_the_same_values_across_locales() {
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let types: HashSet<_> = (0..4).map(|_| unique.bloodType().unwrap()).collect();
        assert_eq!(types, HashSet::from(["A", "AB", "B", "O"]));
        assert!(unique.bloodType().is_err());

        let factors: HashSet<_> = (0..2).map(|_| unique.bloodRh().unwrap()).collect();
        assert_eq!(factors, HashSet::from(["+", "-"]));
        assert!(unique.bloodRh().is_err());

        let groups: HashSet<_> = (0..8).map(|_| unique.bloodGroup().unwrap()).collect();
        let expected = types
            .iter()
            .flat_map(|abo| factors.iter().map(move |rh| format!("{abo}{rh}")))
            .collect();
        assert_eq!(groups, expected);
        assert!(unique.bloodGroup().is_err());

        let reset = faker.unique(true);
        assert!(types.contains(reset.bloodType().unwrap()));
        assert!(factors.contains(reset.bloodRh().unwrap()));
        assert!(groups.contains(&reset.bloodGroup().unwrap()));
    }
}
