use super::{DatabaseResult, Faker, UniqueFaker};
use fastrand::u16;

const WINDOWS: [&str; 12] = [
    "Windows NT 6.2",
    "Windows NT 6.1",
    "Windows NT 6.0",
    "Windows NT 5.2",
    "Windows NT 5.1",
    "Windows NT 5.01",
    "Windows NT 5.0",
    "Windows NT 4.0",
    "Windows 98; Win 9x 4.90",
    "Windows 98",
    "Windows 95",
    "Windows CE",
];
const MAC: [&str; 4] = ["Intel", "PPC", "U; Intel", "U; PPC"];
const LINUX: [&str; 2] = ["i686", "x86_64"];
const LANGUAGES: [&str; 3] = ["en-US", "sl-SI", "nl-NL"];

macro_rules! formatters {
    ($($name:ident -> $result:ty = $sample:expr;)*) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $name(&self) -> $result {
                $sample
            })*
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<$result> {
                self.generate(stringify!($name), || self.0.$name())
            })*
        }
    };
}

formatters! {
    windowsPlatformToken -> &'static str = windows();
    macProcessor -> &'static str = mac_processor();
    linuxProcessor -> &'static str = linux_processor();
    macPlatformToken -> String = mac();
    linuxPlatformToken -> String = linux();
    iosMobileToken -> String = ios();
    chrome -> String = chrome();
    msedge -> String = edge();
    safari -> String = safari();
    opera -> String = opera();
    internetExplorer -> String = internet_explorer();
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn userAgent(&self) -> DatabaseResult<String> {
        Ok(match u16(0..6) {
            0 => self.firefox()?,
            1 => self.chrome(),
            2 => self.internetExplorer(),
            3 => self.opera(),
            4 => self.safari(),
            _ => self.msedge(),
        })
    }

    pub fn firefox(&self) -> DatabaseResult<String> {
        let date = self
            .dateTimeBetweenExpressions("2010-01-01", "now")?
            .with_timezone(&self.application_timezone);
        let platform = match u16(0..3) {
            0 => format!("{}; {}; rv:1.9.{}.20", windows(), language(), u16(0..=2)),
            1 => format!("{}; rv:{}.0", linux(), u16(5..=7)),
            _ => format!("{} rv:{}.0", mac(), u16(2..=6)),
        };
        Ok(format!(
            "Mozilla/5.0 ({platform}) Gecko/{} Firefox/{}.0",
            date.format("%Y%m%d"),
            u16(35..=37)
        ))
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn userAgent(&self) -> DatabaseResult<String> {
        self.try_generate("userAgent", || self.0.userAgent())
    }

    pub fn firefox(&self) -> DatabaseResult<String> {
        self.try_generate("firefox", || self.0.firefox())
    }
}

fn windows() -> &'static str {
    WINDOWS[fastrand::usize(..WINDOWS.len())]
}

fn mac_processor() -> &'static str {
    MAC[fastrand::usize(..MAC.len())]
}

fn linux_processor() -> &'static str {
    LINUX[fastrand::usize(..LINUX.len())]
}

fn language() -> &'static str {
    LANGUAGES[fastrand::usize(..LANGUAGES.len())]
}

fn mac() -> String {
    format!(
        "Macintosh; {} Mac OS X 10_{}_{}",
        mac_processor(),
        u16(5..=8),
        u16(0..=9)
    )
}

fn linux() -> String {
    format!("X11; Linux {}", linux_processor())
}

fn ios() -> String {
    format!(
        "iPhone; CPU iPhone OS {}_{} like Mac OS X",
        u16(13..=15),
        u16(0..=2)
    )
}

fn chrome() -> String {
    let platform = match u16(0..3) {
        0 => linux(),
        1 => windows().to_owned(),
        _ => mac(),
    };
    let webkit = format!("{}{}", u16(531..=536), u16(0..=2));
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Chrome/{}.0.{}.0 Mobile Safari/{webkit}",
        u16(36..=40),
        u16(800..=899)
    )
}

fn edge() -> String {
    let webkit = format!("{}.{}", u16(531..=537), u16(0..=2));
    let major = u16(79..=99);
    // FakerPHP concatenates the build directly after the minor zero.
    let version = format!("{major}.0{}.{}", u16(1000..=1146), u16(0..=99));
    let (platform, token) = match u16(0..4) {
        0 => (windows().to_owned(), "Edg"),
        1 => (mac(), "Edg"),
        2 => (linux(), "EdgA"),
        _ => {
            return format!(
                "Mozilla/5.0 ({}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/15.0 EdgiOS/{version} Mobile/15E148 Safari/{webkit}",
                ios()
            );
        }
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Chrome/{major}.0.{}.{} Safari/{webkit} {token}/{version}",
        u16(4000..=4844),
        u16(10..=99)
    )
}

fn safari() -> String {
    let webkit = format!("{}.{}.{}", u16(531..=535), u16(1..=50), u16(1..=7));
    let platform = match u16(0..3) {
        0 => format!("Windows; U; {}", windows()),
        1 => format!("{} rv:{}.0; {}", mac(), u16(2..=6), language()),
        _ => {
            let device = if fastrand::bool() {
                "iPhone; CPU iPhone OS"
            } else {
                "iPad; CPU OS"
            };
            return format!(
                "Mozilla/5.0 ({device} {}_{}_{} like Mac OS X; {}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/{}.0.5 Mobile/8B{} Safari/6{webkit}",
                u16(7..=8),
                u16(0..=2),
                u16(1..=2),
                language(),
                u16(3..=4),
                u16(111..=119)
            );
        }
    };
    let major = u16(4..=5);
    let version = if fastrand::bool() {
        format!("{major}.{}", u16(0..=1))
    } else {
        format!("{major}.0.{}", u16(1..=5))
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/{version} Safari/{webkit}"
    )
}

fn opera() -> String {
    let platform = if fastrand::bool() {
        linux()
    } else {
        windows().to_owned()
    };
    format!(
        "Opera/{}.{} ({platform}; {}) Presto/2.{}.{} Version/{}.00",
        u16(8..=9),
        u16(10..=99),
        language(),
        u16(8..=12),
        u16(160..=355),
        u16(10..=12)
    )
}

fn internet_explorer() -> String {
    format!(
        "Mozilla/5.0 (compatible; MSIE {}.0; {}; Trident/{}.{})",
        u16(5..=11),
        windows(),
        u16(3..=5),
        u16(0..=1)
    )
}
