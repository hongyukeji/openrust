use std::collections::HashSet;

use jiff::civil::Date as CivilDate;
use openrust::database::eloquent::factories::fake;
use regex::Regex;

const WINDOWS: &str =
    r"Windows (?:NT (?:6\.[012]|5\.(?:01?|[12])|4\.0)|98(?:; Win 9x 4\.90)?|95|CE)";
const MAC: &str = r"Macintosh; (?:U; )?(?:Intel|PPC) Mac OS X 10_[5-8]_[0-9]";
const LINUX: &str = r"X11; Linux (?:i686|x86_64)";
const IOS: &str = r"iPhone; CPU iPhone OS 1[3-5]_[0-2] like Mac OS X";
const LANG: &str = r"(?:en-US|sl-SI|nl-NL)";
const WEBKIT: &str = r" \(KHTML, like Gecko\) ";

#[test]
fn faker_user_agents_match_pinned_browser_and_platform_formats() {
    let faker = fake(Some("zh-TW")).unwrap();
    let firefox = Regex::new(&format!(
        r"^Mozilla/5\.0 \((?:{WINDOWS}; {LANG}; rv:1\.9\.[0-2]\.20|{LINUX}; rv:[5-7]\.0|{MAC} rv:[2-6]\.0)\) Gecko/([0-9]{{8}}) Firefox/3[5-7]\.0$"
    ))
    .unwrap();
    let chrome = Regex::new(&format!(
        r"^Mozilla/5\.0 \((?:{WINDOWS}|{MAC}|{LINUX})\) AppleWebKit/(53[1-6][0-2]){WEBKIT}Chrome/(?:3[6-9]|40)\.0\.8[0-9]{{2}}\.0 Mobile Safari/(53[1-6][0-2])$"
    ))
    .unwrap();
    let edge = Regex::new(&format!(
        r"^Mozilla/5\.0 \((?:(?:{WINDOWS}|{MAC})\) AppleWebKit/53[1-7]\.[0-2]{WEBKIT}Chrome/([0-9]{{2}})\.0\.([0-9]{{4}})\.[1-9][0-9] Safari/53[1-7]\.[0-2] Edg|{LINUX}\) AppleWebKit/53[1-7]\.[0-2]{WEBKIT}Chrome/([0-9]{{2}})\.0\.([0-9]{{4}})\.[1-9][0-9] Safari/53[1-7]\.[0-2] EdgA)/([0-9]{{2}})\.0([0-9]{{4}})\.(?:0|[1-9][0-9]?)$"
    ))
    .unwrap();
    let edge_ios = Regex::new(&format!(
        r"^Mozilla/5\.0 \({IOS}\) AppleWebKit/53[1-7]\.[0-2]{WEBKIT}Version/15\.0 EdgiOS/([0-9]{{2}})\.0([0-9]{{4}})\.(?:0|[1-9][0-9]?) Mobile/15E148 Safari/53[1-7]\.[0-2]$"
    ))
    .unwrap();
    let safari = Regex::new(&format!(
        r"^Mozilla/5\.0 \((?:Windows; U; {WINDOWS}|{MAC} rv:[2-6]\.0; {LANG}|(?:iPhone; CPU iPhone OS|iPad; CPU OS) [78]_[0-2]_[12] like Mac OS X; {LANG})\) AppleWebKit/(53[1-5]\.(?:[1-9]|[1-4][0-9]|50)\.[1-7]){WEBKIT}Version/(?:[45]\.(?:[01]|0\.[1-5]) Safari/|[34]\.0\.5 Mobile/8B11[1-9] Safari/6)(53[1-5]\.(?:[1-9]|[1-4][0-9]|50)\.[1-7])$"
    ))
    .unwrap();
    let opera = Regex::new(&format!(
        r"^Opera/[89]\.[1-9][0-9] \((?:{WINDOWS}|{LINUX}); {LANG}\) Presto/2\.(?:[89]|1[0-2])\.([0-9]{{3}}) Version/1[0-2]\.00$"
    ))
    .unwrap();
    let ie = Regex::new(&format!(
        r"^Mozilla/5\.0 \(compatible; MSIE (?:[5-9]|1[01])\.0; {WINDOWS}; Trident/[3-5]\.[01]\)$"
    ))
    .unwrap();
    let start = CivilDate::new(2010, 1, 1).unwrap();
    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    let number = |value: &str| value.parse::<u16>().unwrap();
    for _ in 0..128 {
        let sample = faker.firefox().unwrap();
        let date = CivilDate::strptime("%Y%m%d", &firefox.captures(&sample).unwrap()[1]).unwrap();
        assert!((start..=today).contains(&date));

        let sample = faker.chrome();
        let captures = chrome.captures(&sample).unwrap();
        assert_eq!(captures[1], captures[2]);

        let sample = faker.msedge();
        if let Some(captures) = edge_ios.captures(&sample) {
            assert!((79..=99).contains(&number(&captures[1])));
            assert!((1000..=1146).contains(&number(&captures[2])));
        } else {
            let captures = edge.captures(&sample).unwrap();
            let (major, build) = if captures.get(1).is_some() {
                (1, 2)
            } else {
                (3, 4)
            };
            assert_eq!(captures[major], captures[5]);
            assert!((79..=99).contains(&number(&captures[5])));
            assert!((4000..=4844).contains(&number(&captures[build])));
            assert!((1000..=1146).contains(&number(&captures[6])));
        }
        let sample = faker.safari();
        let captures = safari.captures(&sample).unwrap();
        assert_eq!(captures[1], captures[2]);

        let sample = faker.opera();
        assert!((160..=355).contains(&number(&opera.captures(&sample).unwrap()[1])));
        assert!(ie.is_match(&faker.internetExplorer()));
        let sample = faker.user_agent().unwrap();
        assert!(
            [&firefox, &chrome, &edge, &edge_ios, &safari, &opera, &ie]
                .iter()
                .any(|format| format.is_match(&sample)),
            "{sample}"
        );
    }
}

#[test]
fn faker_platform_tokens_preserve_catalogs_and_unique_history() {
    let faker = fake(None).unwrap();
    let unique = faker.unique(true);
    let windows: HashSet<_> = (0..12)
        .map(|_| unique.windowsPlatformToken().unwrap())
        .collect();
    assert_eq!(windows.len(), 12);
    let format = Regex::new(&format!("^{WINDOWS}$")).unwrap();
    assert!(windows.iter().all(|token| format.is_match(token)));
    assert!(unique.windowsPlatformToken().is_err());
    assert!(faker.unique(true).windowsPlatformToken().is_ok());

    let unique = faker.unique(true);
    let linux: HashSet<_> = (0..2).map(|_| unique.linuxProcessor().unwrap()).collect();
    assert_eq!(linux, HashSet::from(["i686", "x86_64"]));
    assert!(unique.linuxProcessor().is_err());
    let mac: HashSet<_> = (0..4).map(|_| unique.macProcessor().unwrap()).collect();
    assert_eq!(mac, HashSet::from(["Intel", "PPC", "U; Intel", "U; PPC"]));
    assert!(unique.macProcessor().is_err());

    for (format, sample) in [
        (MAC, unique.macPlatformToken().unwrap()),
        (LINUX, unique.linuxPlatformToken().unwrap()),
        (IOS, unique.iosMobileToken().unwrap()),
    ] {
        assert!(
            Regex::new(&format!("^{format}$"))
                .unwrap()
                .is_match(&sample)
        );
    }
    let first = unique.user_agent().unwrap();
    assert_ne!(unique.user_agent().unwrap(), first);
    let first = unique.firefox().unwrap();
    assert_ne!(unique.firefox().unwrap(), first);
}
