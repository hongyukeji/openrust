# 固定 Symfony MIME 映射

`generate_map.py` 从 Symfony Mime v8.0.0 提交 `7576ce3b2b4d3a2a7fe7020a07a392065d6ffd40` 的 `MimeTypes.php` 生成框架内的保序 `MAP` 与 `REVERSE_MAP`。输入 SHA-256 固定为 `c52bc8bfba35f33b7c93537b3b0ddd6ec2776f24c846ae8b33c3e0f507e20d6f`；不接受其他版本悄悄替换。许可见 `src/http/request/mime_map.LICENSE`。

```bash
python3 tools/mime/generate_map.py /path/to/fixed/MimeTypes.php src/http/request/mime_map.rs
rustfmt --edition 2024 src/http/request/mime_map.rs
```

`src/http/request/mime.rs` 的 `public_tables_match_full_php_oracle_including_order` 对照 PHP 8.5 调用固定 Symfony 公开 `getExtensions` / `getMimeTypes` 导出的有序全表摘要：前向 1838 键、2786 值、FNV-1a64 `82dc75fa2b2584b6`；反向 1871 键、2765 值、`fbb3352ec5fb0a5a`。摘要按每个键、NUL、各值、NUL、条目结束字节 `ff` 依声明顺序计算，可检测遗漏、值次序和首选扩展变化。原始 PHP 输出及运行来源见任务验收证据；摘要不替代真实默认检测器及应用注册回归。
