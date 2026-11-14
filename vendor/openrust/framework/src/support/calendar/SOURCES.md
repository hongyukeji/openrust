# Calendar sources

The expression scanner, civil arithmetic, and time-zone resolver were extracted
from OpenRust v0.x commit `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`, under
`vendor/openrust/framework/src/support/calendar/date`.

- PHP 8.5.0 timelib supplies the date-expression grammar and transition rules.
  See `LICENSE.timelib` and `LICENSE.php`.
- The legacy parser was developed against Carbon 3.8.4. See `LICENSE.carbon`.
  The Faker adapter uses its date-expression scanner and omits Carbon input
  coercion, custom date factories, and PHP format-string parsing.
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
