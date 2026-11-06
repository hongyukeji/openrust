# 日期解析的固定来源和再生成

- Laravel `v13.39.0` / `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`：`InteractsWithData.php:335–373`，`HttpRequestTest.php:806–925`。输入 helper 的日期/区间能力仍须完整验收，有限 golden 不代表整个 timelib 语法已完成。
- Carbon `3.8.4` 官方 tag：`https://github.com/briannesbitt/Carbon/tree/3.8.4`。它是 Laravel composer 声明最低版的比较样本，**不是 Laravel resolved/lock 版本**。`Creator.php`、`CarbonInterval.php` 的解析/缺项行为按此样本实现；Copyright (C) Brian Nesbitt，完整 MIT 授权原文见 `LICENSE.carbon`。
- PHP `8.5.0` 官方源码：`https://www.php.net/distributions/php-8.5.0.tar.xz`，归档 SHA256 `39cb6e4acd679b574d3d3276f148213e935fc25f90403eb84fb1b836a806ef1e`。`ext/date/lib/parse_date.re`、`tm2unixtime.c`、`parse_posix.c`、`parse_tz.c` 是 PHP format、relative、timezone 语义来源。源码版权包括 Copyright (c) 2015-2023 Derick Rethans / Copyright (c) 2018 MongoDB, Inc.，发行源码的完整 MIT 授权原文见 `LICENSE.timelib`；原版权声明在此保留。
- PHP 内置 timezone database `2025.2` / IANA `2025b`；现有 `chrono-tz 0.10.4` 的 `prebuilt/directory.rs` 声明 IANA `2025b`。双方数据版本对应，不能据此假称解析器和所有年份行为相同；chrono-tz 生成 transition 在 2099 年后为固定尾偏移，因此本实现增加真实 POSIX 尾规则。
- `tail_rules.rs` 由 PHP 官方 `timezonedb.h` 的 **slim TZif** 表生成：598 个 zone，94 个去重 POSIX footer，每 zone 保留 footer 生效 Unix 秒、首个 transition 及 type-0 LMT offset/abbreviation/isdst。源文件 SHA256 `01d9a2097154f96ae320204d1dad3157652216bd4a81cda2e2ad9100f076dbc1`。无 transition 时 footer 全期适用；否则在最后 transition 起适用，符合 `parse_tz.c` / RFC 8536。历史按原 transition/LMT，未来按 M/J/n 和有符号 transition time 计算，不无条件投影 400 年周期。
- `abbreviations.rs` 从同版官方 `timezonemap.h` 生成：每缩写的首个 offset/isdst 条目，144 个；与真实 PHP `DateTimeZone` 对照一致。该源文件 SHA256 由再生成脚本固定校验。表来自同一 timelib 发行与许可证。
- `civil.rs` 使用带 checked 运算的公历/Unix 日分解，底层 i64 年份与 epoch 不受 chrono 限制。`try_chrono` 是显式且可能失败的适配，不能反过来把 chrono 的年份范围当成 PHP 日期范围。

## 再生成静态数据

先将上述官方 PHP 归档解到临时目录。脚本仅读取显式路径并检查完整 SHA256，不运行 PHP、不读系统 TZ、不创建 root build.rs、不联网或安装软件：

```sh
python3 src/support/calendar/date/generate_tzdata.py \
  /path/to/php-8.5.0/ext/date/lib/timezonedb.h \
  src/support/calendar/date/tail_rules.rs \
  /path/to/php-8.5.0/ext/date/lib/timezonemap.h \
  src/support/calendar/date/abbreviations.rs
rustfmt --edition 2024 src/support/calendar/date/tail_rules.rs src/support/calendar/date/abbreviations.rs
```

输入 hash 或固定数量不匹配时脚本失败，不默默更新来源。升级 PHP/IANA 基线须显式更新版本、hash、fixture 和对应差分验收。

## 合成测试和资源边界

`tests/http/fixtures/php85/calendar-date-golden.json` 的 119 项日期结果来自原 Laravel trait + direct-key fixture + 真实 Carbon 3.8.4 / PHP 8.5.0 `-n`，clock 固定 UTC 2020-01-01T16:30:25.123456；不是完整 Laravel Request/Container/PHPUnit 运行。时区未来 fixture 包含三个具名 zone 在 2101、3000、百万年、1800 和负年的真实 PHP 结果；静态全部 zone/全部 footer 另有解析检查。

源日期字符串、format、显式 timezone 参数的总字节上限为独立 20 MiB 工作预算；它不等于 bag 的 expanded-key 预算。同一次扫描接受的语义 token/修饰操作总数最多 1024 个，包含 now/noop；完整 first-day-of 前缀计一次。所有分支共享同一计数，1023 个前缀加 now 可接受，1024 个前缀加 now 拒绝。使用聚合状态并只规范化输入一次，超过返回安全 413 `input_limits_exceeded`；这是 Rust 常驻进程的资源适配。没有修改进程默认 TZ、全局时钟或调用生产 PHP。

## 生产与测试时钟

`CalendarClock::is_test_clock()` 默认 false，`SystemClock` 使用生产语义；`FixedClock` 明确对应 Carbon test-now 并返回 true，自定义计数时钟可显式选择。上下文转发此模式，每次解析只读一次 now；没有进程全局 test-now。

`calendar-date-production-golden.json` 的 18 项结果在真实 Carbon `setTestNow(null)` 后采集。每项包含调用前后真实 UTC clock 窗口；测试以该项 before 时刻注入非 mock 时钟，缺项的整秒、零微秒与输入时区精确比较。`now` / 保留当前微秒的 relative 分支按真实窗口比较整秒，并核对注入的微秒不会丢失；不把不同调用间数微秒的差距误判为 parser 行为差异。`calendar-date-extreme-golden.json` 是真实原 trait + Carbon 的 !U i64 最小/最大值及双向越界结果，包含极值 ISO 周格式。`calendar-date-grammar-golden.json` 保留额外 80 项固定 production 对照；新增样本不能替代未完成组合语法验收。

`calendar-date-composition-golden.json` 保留同一真实 Laravel trait / Carbon / PHP fixture 的 30 项乱序、相对段位置及重复字段结果；`calendar-date-format-output-golden.json` 保留真实 PHP 的 Swatch beats、DST 和缩写输出。固定缩写 DST 取官方首条 isdst，历史 transition 取同版 chrono-tz isdst，首期 LMT 和 POSIX 尾规则保留各自真实状态。

`calendar-date-lexical-golden.json` 的 43 项与 `calendar-date-clock-lexical-golden.json` 的 15 项来自相同真实 trait/Carbon/PHP 入口；后者 mock clock 为 UTC 2020-06-15T16:30:25.123456，以发现一月一日基准掩盖的 month/year 缺项行为。`lexer.rs` 的 source guard 与 `state.rs` 的 GNU year/time 复用、内联 timestamp reset 按上述固定 parse_date.re 移植；Carbon timestamp chunks 算法按 Timestamp.php:168–189，沿用同目录完整许可。

`calendar-date-byte-guard-golden.json` 的 18 项和 `calendar-date-format-guard-golden.json` 的 19 项/6个格式输出来自相同真实 trait/Carbon/PHP 入口；PHP 异常可能包含截断 UTF-8，仅在 JSON 证据的异常文本中替换非法编码，输入及成功结果不改。覆盖 `?` 单字节、`*` 首字节跳过、`S` 只跳合法后缀且保 EOF 错误、U 的24位输入宽度、默认 timelib 空白/标点/NUL、`#` 的分隔集合、负年 x 及真实秒偏移保留/分钟显示。

`calendar-date-remaining-golden.json` 保留同一真实 trait/Carbon/PHP 的53项输入；`input_type` 与 JSON_PRESERVE_ZERO_FRACTION 显式保留 int/float/string，成功结果独立保存 timezone object name / format e / offset seconds。固定 PHP `ext/standard/math.c` 的6位 HALF_UP 分支移植在 lexer::number_format_timestamp，源 SHA256 `29e4529da7ed5f445781bb7e3ac7fe36defcfb12540863ff1451a5f4cda58d7c`，Copyright (c) The PHP Group，作者 Jim Winstead / Stig Sæther Bakken / Zeev Suraski / Thies C. Arntzen，PHP3.01完整许可见 LICENSE.php。Carbon Creator::rawParse 的构造失败→字符串 change 回退与 Timestamp 的 precision14 中间浮点失败路径分别保留，Rust返回安全typed error而不复刻PHP TypeError文本。

时区 object name 保留固定偏移中的秒；e/P/O按PHP格式显示截到分钟，Z与内部offset保留全部秒。默认文本tz/tzcorrection按源prefix消费；显式timezone构造不受该文本词法限制。特殊weekday使用唯一special类型，按输入顺序覆盖，裸weekday与amount weekday的behavior2保留规则各自对应源分支。负极值timestamp的小数正规化采用显式wrapping_sub，复现固定timelib unsigned秒归一的有限边界；其他运算继续checked且不panic。

`calendar-date-overflow-golden.json` 保留 i128 MAX + 微秒进位的正/负、有/无 @ 四项真实 InvalidFormatException；Rust checked_add 后仍按原字符串fallback路径返回安全HTTP400。remaining fixture 的 `rust_error` 是单独标识的框架合同分类，不是PHP异常名改写；读取器逐项核对status/code，成功结果另核对P/O而不混淆时区身份。
