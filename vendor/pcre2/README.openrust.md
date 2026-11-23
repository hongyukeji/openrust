# OpenRust 的 PCRE2 适配

来源：BurntSushi/rust-pcre2 0.2.11，提交 `9f269262a71d88605c67bd3fcc8633970aa6ad81`。保留原有 Unlicense / MIT 许可文件。

仅为现有 `RegexBuilder` 增加 `dollar_endonly(bool)`，将其转发为原生 `PCRE2_DOLLAR_ENDONLY` 编译选项，默认关闭。框架 验证器的 PHP `D` 修饰符需要此选项；不重写表达式、不引入其他引擎或 FFI 层。多行模式与 `$` 的交互由 PCRE2 本身处理。

来源核对及正反回归见 `.agents/project/audits/框架-code-audit-2026-11-21.md`。上游公开相应构建选项后可移除此本地补丁。
