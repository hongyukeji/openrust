# PHP 8.5 标量、enum 与 clamp 合成差分样本

本目录是项目自造的输入案例及 PHP 8.5.0 CLI `-n` 真实输出，不是上游整段测试文本；来源与 SHA 见 provenance.json。固定 precision=14、64 位整数，启用 filter/json/date；未执行 make install，默认 Cargo 测试不需要 PHP。

scalar-cases.json 的 kind=json 由真实 serde_json 解码，kind=bits 用 IEEE754 big-endian hex 重建；非有限值不能成为 serde_json::Value，要在私有转换函数与公开输入两层分别报告。Warnings 是 PHP 的补充观测，不当成 Rust API 必须模拟的运行器行为。

开发时可用合格 PHP 8.5.0 `-n` 重新生成，工作目录为本目录：

```sh
php -n scalar-golden.php scalar-cases.json
php -n enum-golden.php
php -n clamp-golden.php
```

标量617项中613项走真实serde输入，3项非有限IEEE值只能走私有转换函数，1项指数溢出的JSON解码为PHP接受INF而serde拒绝。两项超i64无符号整数在PHP解码为float、serde保留u64，标量结果另行核对。enum48项中42项可表达为bag输入，4项非有限值验证私有转换，2项PHP Stringable对象不能成为输入Value。clamp306项保留304项数值与类型/bit结果，2项非法默认值在Rust的InputNumber参数处静态拒绝；数字字符串边界在Rust调用方显式构造InputNumber。PHP warning/deprecation以及脚本路径是运行器观测，Rust不模拟它们。

不得用系统旧 PHP 或运行源码 shim 冒充真实差分；这里只证明有限合成案例，不宣称整个 PHP/Carbon 语义域均等价。

追加61项scalar-control语料使用真正SP/TAB/LF/CR/VT/FF/NUL/NBSP/ZWSP/BOM字符，由同一脚本读取scalar-control-cases.json生成；公开Request在禁用规范化后逐项比较四种转换。它们不替换原617项，避免把其中字面反斜线转义文本当成真实控制字符覆盖。
