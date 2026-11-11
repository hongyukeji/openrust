use super::*;

#[test]
fn rotates_on_period_boundary_and_keeps_only_matching_files() {
    let root = std::env::temp_dir().join(format!(
        "openrust-rotation-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    fs::create_dir(&root).unwrap();
    let make = |name, period, max| {
        RotatingFile::new(
            FileOptions {
                path: root.join(name),
                permission: None,
                locking: true,
            },
            period,
            max,
        )
        .unwrap()
    };
    let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
    let daily = make("access.log", Period::Daily, 2);
    fs::write(root.join("access-2025-12-31.log"), "old").unwrap();
    fs::write(root.join("access-2026-02-30.log"), "invalid").unwrap();
    fs::write(root.join("access-2026-01.log"), "monthly").unwrap();
    fs::create_dir(root.join("access-2025-12-30.log")).unwrap();
    daily.write(date(2026, 1, 1), b"first").unwrap();
    daily.write(date(2026, 1, 2), b"second").unwrap();
    daily.write(date(2026, 1, 2), b"+same").unwrap();
    assert!(!root.join("access-2025-12-31.log").exists());
    assert_eq!(
        fs::read(root.join("access-2026-01-01.log")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(root.join("access-2026-01-02.log")).unwrap(),
        b"second+same"
    );
    assert!(root.join("access-2026-02-30.log").is_file());
    assert!(root.join("access-2026-01.log").is_file());
    assert!(root.join("access-2025-12-30.log").is_dir());

    let latest = make("latest.log", Period::Daily, 1);
    latest.write(date(2026, 1, 2), b"new").unwrap();
    latest.write(date(2026, 1, 1), b"late").unwrap();
    assert!(!root.join("latest-2026-01-01.log").exists());
    assert_eq!(
        fs::read(root.join("latest-2026-01-02.log")).unwrap(),
        b"new"
    );

    let monthly = make("worker", Period::Monthly, 0);
    monthly.write(date(2026, 1, 1), b"first").unwrap();
    monthly.write(date(2026, 1, 31), b"+same").unwrap();
    monthly.write(date(2026, 2, 1), b"next").unwrap();
    assert_eq!(
        fs::read(root.join("worker-2026-01")).unwrap(),
        b"first+same"
    );
    assert_eq!(fs::read(root.join("worker-2026-02")).unwrap(), b"next");
    fs::remove_dir_all(root).unwrap();
}
