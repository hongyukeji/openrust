# Rule-based number formatting

The Rust interpreter in `rbnf/` derives from the MIT-licensed [verbalize 0.2.0](https://github.com/relu/verbalize/tree/fe2424ecd479d0e394e3c3e88b3bbbb752323264) interpreter. OpenRust keeps the rule whitespace and soft hyphens and adds ICU denominator selection, localized decimal fallback, non-finite values and locale-specific default rules.

`rules.json` stores ICU's default public rule set followed by its ordered rule groups. A group contains its name and a sequence of descriptor/body pairs. Data comes from [CLDR 46 spellout XML](https://github.com/unicode-org/cldr/tree/release-46/common/rbnf) and [ICU 76.1](https://github.com/unicode-org/icu/tree/release-76-1/icu4c/source/data/rbnf). ICU supplies resolved resources that lack equivalent CLDR files and the Indonesian rule text. The Macau and Taiwan overlays preserve unspaced digits in the traditional Chinese decimal rules.

`locales.json` maps ICU 76.1 number-formatting locales to their actual spellout resource and decimal symbols. Each symbol array contains the decimal separator, grouping separator, minus sign, zero digit, infinity and NaN. The finite fixture comes from Laravel v13.40.0 running PHP 8.5.12 with ICU 76.1. The non-finite fixture comes from ICU 76.1 rule-based formatting.

The included MIT and Unicode licenses cover the interpreter and its data. OpenRust parses and evaluates the data in Rust.
