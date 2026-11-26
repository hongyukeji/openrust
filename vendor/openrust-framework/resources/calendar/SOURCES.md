# Fixed timezone data

The framework resolves configured IANA names with Jiff 0.2.46's native
`TimeZone::tzif`. The checked-in `tzif.data` and `tzif.rs` contain IANA **2025b**
vanguard TZif compiled by the matching upstream `zic`, using its default
`backward` aliases without optional `backzone` history, as chrono-tz 0.10.4 did.
Equal TZif payloads share one byte range; 598 names map to 341 payloads,
203,282 bytes in total. The name set is the existing PHP 8.5.0 / IANA 2025b
Calendar baseline in `src/support/calendar/tail_rules.rs`.

Official source inputs:

| Archive | SHA-256 |
| --- | --- |
| [tzdata2025b.tar.gz](https://data.iana.org/time-zones/releases/tzdata2025b.tar.gz) | `11810413345fc7805017e27ea9fa4885fd74cd61b2911711ad038f5d28d71474` |
| [tzcode2025b.tar.gz](https://data.iana.org/time-zones/releases/tzcode2025b.tar.gz) | `05f8fedb3525ee70d49c87d3fae78a8a0dbae4fe87aa565c65cda9948ae135ec` |

The files are public domain; see [LICENSE.iana](LICENSE.iana), copied unchanged
from the official archive. The upstream build uses only `zic.c` and its headers;
the BSD-licensed optional files listed in that license are not used or shipped.

From this directory, regenerate with Python 3.12+, make, awk and a C compiler:

```sh
python3 generate_tzdb.py /path/to/tzdata2025b.tar.gz /path/to/tzcode2025b.tar.gz
```

The generator verifies both archive hashes and the version before extracting
them into a temporary directory. It runs the upstream commands:

```sh
make zic vanguard.zi PACKRATDATA= PACKRATLIST=
./zic -b slim -d /temporary/zoneinfo vanguard.zi
```

Generation does not read the host timezone database, run Cargo, or add a build
script. `tzif.rs` ordering is stable and both generated outputs are reproducible.
Do not change the formatting of generated `tzif.rs`.

Jiff's official `jiff-tzdb` 0.1.4 also uses IANA 2025b, but deliberately ships
**rearguard** data. That data changes negative daylight saving flags, including
Dublin, Casablanca, Prague and Windhoek. The framework retains the existing
Chrono/timelib vanguard semantics for Calendar's PHP `I` output and explicit
daylight-saving preference in folds. The timezone algorithm remains Jiff's;
the generator only compiles and deduplicates upstream data. See the official
[jiff-tzdb explanation](https://docs.rs/jiff-tzdb/0.1.4/jiff_tzdb/).

When upgrading IANA, update these hashes and the Calendar first-transition and
POSIX tail records, abbreviation table, and Faker timezone/country list
together. Validate historical offsets, negative DST and folds against the
fixed PHP baseline before changing the supported behavior.

The optional `backzone` history changes historical alias behavior: for example,
Accra would acquire its own historical offsets and daylight-saving transitions
instead of the previous Abidjan history. It is deliberately excluded to preserve
the existing native date behavior. Calendar retains its separate PHP first-record
and POSIX-tail handling unchanged.
