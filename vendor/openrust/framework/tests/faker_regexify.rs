use openrust::database::{DatabaseError, DatabaseResult};

#[test]
fn faker_regexify_generates_matching_strings_and_rejects_unsupported_patterns() -> DatabaseResult<()>
{
    let faker = openrust::database::eloquent::factories::Faker::new("en-US")?;
    for pattern in [
        r"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,4}",
        r"(AB|XY){1,3}-\d{4}\.?\w*",
        r"\$\/[.]\\[0-9]{0,2}",
        r"(A|B)?(\d|[a-f]){2}.",
    ] {
        let verifier = regex::Regex::new(&format!(r"\A(?:{pattern})\z")).unwrap();
        for _ in 0..64 {
            let sample = faker.regexify(pattern)?;
            assert!(verifier.is_match(&sample), "{pattern}: {sample:?}");
        }
    }
    assert_eq!(faker.regexify(r"/^AB\d{0}$/")?, "AB");
    assert_eq!(faker.regexify("")?, "");
    for invalid in [
        r"[",
        r"a{3,1}",
        r"[^ab]",
        r"\p{Greek}",
        r"a{2,}",
        r"(?=A)",
        r"(a)\1",
        r"(?i)a",
        r"ab^cd",
        r"a+?",
    ] {
        assert!(faker.regexify(invalid).is_err(), "{invalid}");
    }
    assert!(matches!(
        faker.regexify("(A{100}){100}"),
        Err(DatabaseError::InvalidQuery(
            "Faker regex exceeds generation limits"
        ))
    ));
    let unique = faker.unique(false);
    assert_eq!(unique.regexify("AB")?, "AB");
    assert!(unique.regexify("AB").is_err());
    assert_eq!(faker.unique(true).regexify("AB")?, "AB");
    Ok(())
}
