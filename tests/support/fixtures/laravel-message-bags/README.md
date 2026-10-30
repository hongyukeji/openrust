# Laravel 固定行为样本

来源：`laravel/framework` commit `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`，上游 [MIT 许可](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/LICENSE.md)。此处只保存**输出**；未复制上游源码。原始执行证据：`/tmp/openrust-c04d10-reference/provenance.json`，真实 PHP 8.5.0 CLI `-n` 分别运行 `reference.php` 和 `json_flags.php`，两次 exit 0、stderr 空；直接调用固定上游类，不启动 Laravel 应用或 PHPUnit。

| 文件 | 行数 | SHA-256 |
| --- | ---: | --- |
| `output.jsonl` | 10 | `843707a9b6c98dff7b431cfc46bfd19168da02a89da8c0584e7dbbc65e959824` |
| `edge_output.jsonl` | 31 | `5e271f897078416df5879424cad46a3de6ab1efbbb24921b21c8c99e49b36edc` |
| `json_flags.jsonl` | 6 | `8073db17f52dc3d812dcd82ea2e39c821fc8019c89f97415e1854e1ff97c37a6` |
| `sparse_format_bare.jsonl` | 45 | `5978fca313e7077f6786770a8589b6dfd43eff31703cb0c7db3ea0fbf50e37d5` |
| `sparse_format_laravel_error.jsonl` | 45 | `f3cf6374d769bd83eac51a9c3207ab889ddd5a804b2309d7565a358829bb6966` |

本机新取证统一从 `/Users/Home/Codes/Git/laravel-framework-baseline` 读取上述固定 commit 的真实类及依赖，不再读取旧 `/tmp` 源码；先校验 commit、工作区与17个依赖哈希，再用固定 PHP 8.5.0 CLI 执行参考脚本。可复跑脚本默认根已切换该基线目录；历史脚本、输出及 provenance 保留原路径事实，不据目录迁移重写来源。仓库测试只读取静态原始输出，不依赖本机 PHP 或 `/tmp` 状态。`json_flags.jsonl` 原始六行每行第三项是 PHP mixed `float(1.0)`，Rust 构造入口依正式 docblock 只接 String；测试保留并核对原始第三项 `1` 或 `1.0`，再从原始 JSON 截取前两条 String 项作受支持子例比较。该子例不是原样三项通过；PHP mixed 第三项仍是公开类型边界。

Edge 样本由相同固定类和 PHP 8.5.0 CLI 直接运行 `edge_reference.php` 得到：exit 0、stderr 空；`/tmp/openrust-c04d10-reference/edge_provenance.json` 记录脚本与依赖哈希。内层命名键冲突会生成嵌套数组；这是 `String` 消息构造输入以外由 merge 派生的状态，需单独验收。

稀疏键与嵌套格式样本由 `sparse_format_reference.php` 在相同固定源码与 PHP 8.5.0 CLI `-n` 下分别以 `bare`、`laravel_handle_error_method` 参数运行；两次均 exit 0，每次输出 45 个具名结果，来源脚本、17 个源码 SHA 与运行记录见 `/tmp/openrust-c04d10-reference/sparse_format_provenance.json`（SHA-256 `19f0a0385cd4d3b422254ad1cef81edf4dd187d42cbfb9f8ded0955a8a8ee447`）。仓库内原始 JSONL 不改写。裸 PHP 的非默认格式对嵌套数组警告后输出 `Array`；固定 Laravel `HandleExceptions::handleError` 将该警告转为 `ErrorException`。Rust 用 `Result` 对应受管错误，测试同时保留并分类裸 PHP 输出。外层数值键 merge 重编号，内层已存稀疏数值键在递归合并时保持原键。
