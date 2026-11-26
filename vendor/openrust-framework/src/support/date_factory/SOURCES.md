# 日期工厂来源

日期ID的公开职责按固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `Support/Carbon.php::createFromId`。

UUID/ULID解析复用框架已有 `uuid 1.29.2` 和 `ulid 1.2.1`；没有新增生产依赖。UUID微秒转换的Rust适配参考 [ramsey/uuid 4.7.6](https://github.com/ramsey/uuid/tree/4.7.6)，提交 `91039bc1faa45ba123c4328958e620d382ec7088` 的 `Converter/Time/{PhpTimeConverter,GenericTimeConverter}.php`。保留默认14位double文本尝试与精度不足时的整数half-up后备、v2低32位清零及非时间ID错误。必要的MIT原文见 `LICENSE.ramsey-uuid`。

其余格式、表达式、分量创建与时区运算共用 [Calendar来源说明](../calendar/SOURCES.md) 及既有实现。
