# Request URL 查询串真实参考

这组 fixture 使用实际 PHP 8.5.0 CLI、固定 Symfony `HeaderUtils::parseQuery`、PHP `ksort` / `http_build_query` 和固定 Laravel `Arr::query` / `Arr::except` 生成。没有为核心规则写 stub，也没有执行完整 Laravel 应用或官方 PHPUnit 套件。

每条 Symfony 线执行 198 条 canonical 与 7 条数组操作。196 条 canonical 对照字符串结果；另 2 条非法 UTF-8 验证 Rust 的 HTTP 400 `input_encoding_invalid` 表示适配，不计作 PHP 字节等价。两条线的结果已实际生成并比较一致，PHP 警告为 0。

`cases.json` 保存全部实际输入；混合排序覆盖 3 / 4 / 5 / 6 / 16 / 17 / 127 / 128 / 1024 条顶层键、数字字符串、整数溢出、非全序组合及确定性随机语料。另含嵌套顺序、点与空格、畸形括号、逐字段 NUL、数组覆盖、增删参数、null / bool / float。原始 NUL 只用于直接调用内部函数的边界检查，HTTP URI 不接受该字符。

运行时显式设置 `max_input_vars=4096`，以免 1024 条字段参考被 PHP 默认输入限制截断；这不是 PHP 默认值一致性证明。`precision=14`、嵌套限制和运行参数在 golden 元数据及 `reference-run.json` 中保留。Rust 的原始 query 16 KiB、1024 字段、128 KiB 输出及输入树限制是公开资源边界，超限返回 413。

固定官方 source URL、commit、SHA256、CLI 校验值和已核 PHP release 归档校验值见 `provenance.json`。本次复用了此前构建且 SHA256 一致的 PHP 8.5.0 CLI，重新执行新语料；本次启动的新构建已停止，没有安装全局软件。PHP release 内的三份关键排序源码与固定 commit 哈希交叉核对一致。仓库不保存 PHP / Symfony / Laravel 全量源码或本机二进制。

## 复跑

提供已信任的 PHP 8.5.0 CLI 与任务专属缓存/输出目录：

```sh
python3 tests/http/fixtures/laravel-request-url/generate.py \
  --php /path/to/php85 \
  --source-dir /tmp/task-owned-url-sources \
  --output-dir /tmp/task-owned-url-output \
  --php-sha256 trusted-cli-sha256
```

默认 CLI 校验值为本次实测二进制；使用另一份经过信任流程构建的 CLI 时，显式传入其可信 SHA256。生成器检查版本、CLI 校验值和每份官方固定源码的校验值；缺失 source 从官方固定 commit 下载，已有但哈希不符则直接拒绝。两个 golden 及运行报告生成在指定输出目录，不依赖旧 `/tmp` 路径，也不调用系统 `php`。

普通 Cargo 单测只读取仓内 golden，不需要 PHP、网络或 `/tmp`。具名测试 `canonical_and_array_operations_match_real_php85_symfony_both_lines` 逐 ID 执行全部 canonical / encode / merge / without 操作，并分别核对 UTF-8 适配及删除后的非空状态。
