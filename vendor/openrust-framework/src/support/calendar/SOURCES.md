# Calendar sources

The expression scanner, civil arithmetic, and time-zone resolver were extracted
from OpenRust v0.x commit `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`, under
`vendor/openrust-framework/src/support/calendar/date`.

- PHP 8.5.0 timelib supplies the date-expression grammar and transition rules.
  See `LICENSE.timelib` and `LICENSE`.
- The legacy parser was developed against Carbon 3.8.4. See `LICENSE.carbon`.
  The Faker adapter uses its date-expression scanner and omits Carbon input
  coercion, custom date factories, and PHP format-string parsing.
- Request date input reuses the v0.x format scanner and scalar timestamp
  conversion. The two callers share civil arithmetic and time-zone resolution.
- Request interval input reuses the v0.x interval parser and its PHP 8.5.0 and
  Carbon 3.8.4 comparison fixtures.
- PHP date output reuses the OpenRust v0.x renderer from commit `44632383`.
  The token behavior is checked against PHP 8.5.0's `ext/date/php_date.c`.
- The checked-in zone data comes from PHP 8.5.0's IANA 2025b snapshot.
  `generate_tzdata.py` verifies the source checksums before regeneration.

From this directory, run the following against the extracted PHP 8.5.0 sources.

```sh
python3 generate_tzdata.py /path/to/php-8.5.0/ext/date/lib/timezonedb.h tail_rules.rs \
  /path/to/php-8.5.0/ext/date/lib/timezonemap.h abbreviations.rs
rustfmt --edition 2024 tail_rules.rs abbreviations.rs
```

The same generator also writes `identifiers.rs` with each identifier's canonical
flag and two-letter country code from the verified PHP timezonedb header.
Validation's timezone rule uses this metadata, including aliases and country
filtering, without consulting the host database or changing Calendar's version.

- Date factory timestamp creation is additionally compared with Carbon 3.10.3,
  commit `8e3643dcd149ae0fe1d2ff4f2c8e4bbfad7c165f`,
  `src/Carbon/Traits/Timestamp.php`. The fixed numeric chunk parser and PHP
  number-format rounding are reused for seconds and milliseconds; negative
  millisecond parts retain that version's behavior. The source remains MIT
  licensed under `LICENSE.carbon`.

- Date factory format probes reuse the `regexFormats` and
  `regexFormatModifiers` tables and replacement order from the same fixed
  Carbon `src/Carbon/Factory.php`. The immutable tables are published in
  `resources/calendar/date-format-patterns.json` and evaluated with the
  framework's existing PCRE2 dependency. Relative-expression probes reuse
  Calendar with the two reference instants from Carbon `Traits/Date.php`.
