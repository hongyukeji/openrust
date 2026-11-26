use std::cell::Cell;

use openrust::database::{DatabaseError, eloquent::factories::fake};

#[test]
fn biased_numbers_respect_the_bounds_and_curve() {
    let faker = fake(None).unwrap();
    let mut low_sum = 0;
    let mut high_sum = 0;
    for _ in 0..512 {
        let low = faker
            .biased_number_between(Some(-50), Some(50), Some(&|x| 1.0 - x))
            .unwrap();
        let high = faker
            .biased_number_between(Some(-50), Some(50), Some(&|x| x))
            .unwrap();
        assert!((-50..=50).contains(&low) && (-50..=50).contains(&high));
        low_sum += low;
        high_sum += high;
        assert!((0..=100).contains(&faker.biased_number_between(None, None, None).unwrap()));
    }
    assert!(low_sum < 0 && high_sum > 0);
    for endpoint in [i64::MIN, i64::MAX] {
        assert_eq!(
            faker
                .biased_number_between(Some(endpoint), Some(endpoint), None)
                .unwrap(),
            endpoint
        );
    }
    assert!(faker.biased_number_between(Some(1), Some(0), None).is_err());
    assert!(
        faker
            .biased_number_between(None, None, Some(&|_| f64::NAN))
            .is_err()
    );
    assert!(
        faker
            .biased_number_between(None, None, Some(&|_| 0.0))
            .is_err()
    );
    let unique = faker.unique_with_retries(true, 1);
    assert_eq!(
        unique
            .biased_number_between(Some(7), Some(7), None)
            .unwrap(),
        7
    );
    assert!(
        unique
            .biased_number_between(Some(7), Some(7), None)
            .is_err()
    );
    assert_eq!(
        faker
            .unique_with_retries(true, 1)
            .biased_number_between(Some(7), Some(7), None)
            .unwrap(),
        7
    );
}

#[test]
fn optional_and_valid_preserve_lazy_defaults_errors_and_retry_boundary() {
    let faker = fake(None).unwrap();
    assert_eq!(
        faker
            .optional(Some(0.0), Some(99), |_| panic!("formatter ran"))
            .unwrap(),
        Some(99)
    );
    assert_eq!(
        faker
            .optional(Some(0.0), None, |_| -> Result<u8, _> {
                panic!("formatter ran")
            })
            .unwrap(),
        None
    );
    assert_eq!(
        faker
            .optional(Some(100.0), None, |faker| Ok(faker.number_between(8, 8)))
            .unwrap(),
        Some(8)
    );
    assert!(matches!(
        faker.optional(Some(1.0), None, |_| Err::<u8, _>(
            DatabaseError::InvalidQuery("fixture")
        )),
        Err(DatabaseError::InvalidQuery("fixture"))
    ));

    let calls = Cell::new(0);
    let generate = |_: &openrust::database::eloquent::factories::Faker| {
        calls.set(calls.get() + 1);
        Ok(calls.get())
    };
    assert_eq!(
        faker.valid(|&value| value == 2, Some(2), generate).unwrap(),
        2
    );
    calls.set(0);
    assert!(faker.valid(|&value| value == 2, Some(1), generate).is_err());
    assert_eq!(calls.get(), 2);
    calls.set(0);
    assert!(
        faker
            .valid(|_| panic!("validator ran"), Some(0), generate)
            .is_err()
    );
    assert_eq!(calls.get(), 1);
}
