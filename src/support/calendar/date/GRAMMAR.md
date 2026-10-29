# 固定 timelib production 覆盖矩阵

固定源码：PHP 8.5.0 `ext/date/lib/parse_date.re`，来源和许可见 SOURCES.md。下列“实作已落”仅说明代码路径和样本已建立；每轮差分以实际 Cargo 结果为准；当前证据入口与组合限制见下文，不以历史快照替代新改动验收。

| 固定 production / token 家族 | 当前实现路径 | 差分与剩余边界 |
|---|---|---|
| hour24/hour12/minute/second/frac/meridian | 唯一 format::Cursor + lexer 源 production scanner，hour24=24/second=60归一 | 原119及 grammar timetiny/timeshort/timelong；时间点/冒号和 a.m./p.m.已落，混合来源的双time检测须继续完整验证 |
| year/year2/year4/year4withsign/yearx | signed整体 i64/两位年窗口/扩展civil | 69/70年、百万年、!U极值；x/X全部19位边界组合仍需扩展源级corpus |
| day/month/dayofyear/weekofyear | 日期分量宽松归一、ISO ordinal1-based、format z0-based | day0/month0及长溢出接受边界须按全部 source guards继续核验，不能用chrono拒绝替代 |
| monthfull/monthabbr/monthroman/daysuf | 英文/罗马月份、数字后ordinal suffix剥离 | grammar datefull/datefull_nosep/monthroman；suffix错误及字节级分隔组合仍须验收 |
| timetiny12/timeshort12/timelong12 | ga/g.ia/g:i:sa/g.i.sa等源等价format分支 | grammar三项和原meridian测试；零长度separator及大小写全组合不由单例证明 |
| timetiny24/timeshort24/timelong24/iso8601long/iso8601normtz | t前缀、dot/colon、fraction与尾zone的共享scanner | grammar五项；乱序和重复字段已接入同一聚合状态，30项composition矩阵已执行；组合边界见下文 |
| gnunocolon/iso8601nocolon | 4位 HHMM优先，6位 HHMMSS，mock微秒分别处理 | grammar1630/2020/163025；Ymd宽度不会再误吞6位时间 |
| americanshort/american/dateslash/iso8601dateslash | m/d、m/d/Y、Y/m/d及尾slash | grammar四项；双date拒绝和 separator 微妙优先级须继续corpus |
| iso8601date4/iso8601date2/iso8601datex/gnudateshorter/gnudateshort | source year/year2/year4/yearx 的宽度与年窗口/年月缺日规则 | grammar五项；不同分隔导致的两位年路径已落，歧义组合仍需核验 |
| pointeddate4/pointeddate2 | 4位年份与dot time优先级 | 3.2.20实际为03:02:20而非日期；3-tab-2.2020日期样本已落 |
| datefull/datenoday/datenodayrev/datetextual/datenoyear/datenoyearrev/datenocolon | 同一 source scanner 的月份/数字/分隔 guard，mock无年保time | grammar全部代表；完整分隔字符闭包仍待验收；时间/日期乱序聚合与矩阵已执行 |
| soap/xmlrpc/xmlrpcnocolon/wddx/exif | Ymd、T/t、fraction、变宽数字共享scanner | grammar五项、原ISO样本；非法double separator/重复字段规则继续核验 |
| pgydotd/pgtextshort/pgtextreverse | numeric string先走固定 Carbon chunks 前置；文本日期共享 source scanner | 2020.034实际timestamp，不以PHP core日期优先；三样本已落 |
| mssqltime | 12小时/秒后冒号fraction+meridian | grammar4:30:25:123PM；更宽fraction和separator组合继续corpus |
| isoweekday/isoweek | compact/dashed ISO week、周一基准 | grammar2020W013/2020W01；ISO输出周以i128日计算，不跨i64instant |
| firstdayof/lastdayof/weekdayof | 唯一 ParseState 聚合状态，绝对month共享日期parser，正/last ordinal weekday | grammar绝对/相对月份；modifier token任意位置已汇入同一状态，矩阵已执行，组合边界见下文 |
| backof/frontof | +/-15分钟及meridian | grammar7pm两项；输入混合date/time时序仍待完整source corpus |
| clf/timestamp/timestampms | 任意位置zone共享状态，@数值/十进制微秒 | 原CLF+RFC修复、grammar@正/负/小数；half-up/typed极值与chunks代表已差分，组合边界见下文 |
| reltextnumber/reltexttext/reltextunit/relnumber | ordinal 1–12、next/last/previous/this、单位/多符号/符号后空格 | grammar多项；µs/非ASCII unit字节边界、全部relative别名仍需继续对齐 |
| relative/relativetext/relativetextweek | 分量先累加再归一、ago、week对齐周一、weekday特殊算术 | grammar正负weekday/周末/月底/nextweek；多组weekday特殊状态与普通分量的语义顺序仍需源级验证 |
| now/today/yesterday/tomorrow/noon/midnight | 显式clock读取一次，单token重置规则 | 原119/生产18；重置时间的先后顺序、双time和绝对/relative乱序已落并执行矩阵 |
| tz/tzcorrection | 全598 IANA/POSIX/LMT、144abbr、offset、DST gap/fold | 原119/30future+元数据；GMT前缀offset、括号abbr、二次warning/三次error已落，全部have_zone/dst组合仍须验收 |
| PHP format default_format_map + Carbon c/meridian修正 | 独立format scanner，!全reset/\|补缺项/U/escape/+等 | 原119、80grammar格式扩展、生产18、极值4；?/*字节、S/#/U/x及Unicode代表差异已修并回归 |

## 实现与验收边界

默认日期由 `lexer.rs` 直接读取固定 source guard，绝对日期、时间、relative、zone 汇入唯一 `ParseState`，扫描后统一执行 special-early / weekday / normalize / relative / first-last / special-weekday / zone。旧即时 apply 与默认 DATE/TIME_FORMATS 候选路径已移除；显式 createFromFormat 保持独立的宽松格式合同，不用 chrono 的范围或有限格式候选替代 PHP 解析。

本簇最终日期22项、公开Request/Fluent34项及关联请求/路由、唯一模块、workspace/feature/MSRV、真实双库、生成应用和真实TCP均已执行，结果见 `.agents/project/memory/testing.md` 的 C04-B2。代表参考保留原119、grammar80、composition30、lexical43、六月clock15、生产clock18、极值4、named-zone30与598 footer；新增byte18、format19和6输出、remaining53及overflow4均已接入。不是只编译或只运行 PHP。

- 默认 source scanner 保留数字宽度、年窗口、GNU年份/时间复用、长fraction、CLF/EXIF、ISO week及内联timestamp reset。
- 默认分隔按源空白/标点/NUL消费，不删除NUL拼接token；显式 `?/*` 按byte消费，`S`只消费合法后缀并保EOF错误，`#`采用源集合，`U`支持24位宽度，负年x按源补位。
- PHP六位half-up、int/float/string入口与字符串构造失败回退分开；小数进位用checked_add，越界安全返回而不panic。明确有限timelib归一边界才使用wrapping，其他运算继续checked。
- 单一Special按源顺序覆盖，amount weekday与裸weekday分别对应behavior规则；默认文本TZ prefix与显式timezone options分流。时区身份保秒，e/P/O按分钟显示，Z保秒。
- remaining53保留input_type/JSON_PRESERVE_ZERO_FRACTION；36成功独立断name/e/P/O/offset，17错误逐项断HTTP status/code。overflow四参考和公开Request/Fluent六字符串验证安全错误及bag不变。完整来源与许可见SOURCES.md。

## 固定分支覆盖与组合限制

不同模型只读按下列五类固定源码核对，均找到对应实现，未取得新增缺失production的可复现反例。不能将组合空间未穷举写成某个完整分支未实现，也不能把Request日期输入责任移到R12全Carbon工具箱。

| 核对项 | 对应实现 | 当前证据边界 |
| --- | --- | --- |
| 默认分隔与guard | lexer主循环、numeric/text/time scanner、parse入口 | 代表正负例已差分；不宣称穷尽所有字符组合 |
| weekdayof/firstdayof/lastdayof | relative扫描与唯一state special覆盖 | 代表乱序、覆盖及普通分量组合已差分；不宣称穷尽任意token序列 |
| tz/tzcorrection | lexer prefix与zone options分流 | 代表消费长度、括号、错误及秒偏移已差分；不将两种入口混同 |
| Carbon timestamp chunks | lexer chunks、half-up与parse回退 | 类型、舍入、极值和进位负例已差分；不把所有文本当f64 |
| PHP default_format_map | format独立switch | 固定格式分支及代表byte/Unicode/EOF组合已差分；不以有限样本证明全体字符串 |

上表中的组合边界用于后续回归与上游跟进，不将未知组合默认为通过。发现具体源反例时补回归并修实现；资源上限单列SOURCES.md，不把有效语法随意列为不适用。本页仅覆盖Request的日期读取，完整Carbon工具箱另按既有矩阵交付。
