# 欢迎页文档语言目录

## 范围与决定

用户要求去掉视图中的固定三语映射，并尽量保留视图职责。视图通过当前语言的 `welcome.documentation_locale` 读取文档目录；该语言没有此键时使用 `en`，不借用文案回退语言的目录。链接的仓库地址和章节路径仍在视图中统一组装。

框架十八语及应用三语欢迎页资源增加此键，值对应现有文档目录。新增其他语言的文档后，可在应用对应 `welcome.json` 覆盖，例如 `"documentation_locale": "ja"`。这不会改变应用配置的语言候选、当前语言或响应语言。没有修改控制器、路由、配置加载与核心译器。

沿用现有按键翻译与应用覆盖机制，未增加目录扫描、构建生成器或新的配置项。参考[官方语言资源与翻译读取说明](https://laravel.com/framework/docs/13.x/localization#retrieving-translation-strings)；此文档链接键为欢迎页自身资源，不声明其为固定上游接口。

## 实际验收

隔离源码及日志位于项目 `target/tmp/welcome-documentation-links-20261123/`。复用已确认闲置的 `target/tmp/model-fields-sync-20261123/cargo-target`，通过项目构建守卫运行：

```sh
python3 -B tests/build_guard.py \
  --workflow welcome-documentation-links-20261123 \
  --target-dir target/tmp/model-fields-sync-20261123/cargo-target -- \
  cargo +1.94.0 test --locked --offline \
  --manifest-path target/tmp/welcome-documentation-links-20261123/source/Cargo.toml \
  --test welcome
```

- `welcome-tests-final2.log`：35 项通过、0 失败、0 忽略。通过真实 HTTP Kernel 渲染验证链接、语言选择、文案回退及应用单键覆盖。日语资源指定 `ja` 后，两处文档链接均使用 `ja`，响应语言仍为 `ja`。
- `resources.json`：21 份欢迎页资源均可解析，随附目录指向的目录页和部署章节均实际存在。
- `accepted-inputs.json`：模板、测试及21份资源与通过验收的快照逐文件一致。
- `rustfmt --check` 与本批相关路径的 `git diff --check` 通过。

初轮发现未知语言使用中文文案回退时错误地借用了中文文档目录，已补精确语言存在性检查并通过回归。中间尝试完整三元表达式时出现模板编译错误；现有表达式解析器尚不支持完整 `condition ? a : b`，本批使用既有 `@if`／`@else` 指令，不扩展表达式语法。该语法缺口保留供 Blade 功能簇后续核对，不宣称已闭合。

本批未启动 HTTP 监听服务或 Docker 资源。保留可复用编译缓存及全部验收日志，该轮结束时可用空间约39.4 GiB，尚未暂存、提交或推送。没有重新进行浏览器视觉验收或生产部署验收。

## 应用名称参数化（同日用户追加）

用户明确指定简体中文介绍为 `使用 :name 构建你的应用。`。同步框架十八语与应用三语的介绍，占位符统一为 `:name`；视图通过 `config('app.name')` 传入实际应用名称。控制器和配置逻辑未改。

`description-tests.log`：当前组合36项欢迎页HTTP测试全部通过。新增测试逐一加载十八语资源，用实际 `.env` 的 `APP_NAME="My <Blog> & Co"` 经配置加载、HTTP Kernel与Blade渲染，确认每种语言只含一个名称占位符、名称正确替换、输出转义且没有遗留 `:name`。

提交只包含本批欢迎页模板、三语应用资源、十八份框架welcome资源、相关测试、三语使用说明及规范对应段落。其他十八语核心加载与词典、品牌清理和原有已暂存的许可证变更保留给所属批次。

独立提交组合以已提交HEAD为基础，仅叠加本批28个路径，保留原始语言名资源引用及默认候选测试，不包含其他协作者的with_defaults加载与语言词典。`commit-tests-final.log`：36项HTTP测试全部通过、0忽略。缺少文档语言词条时使用英文目录的原有断言已按本次约定更新；完整在途组合仍由`description-tests.log`验证。新增三语说明、测试格式与本批差异检查通过。

本批完成后未启动任何服务或Docker资源，保留同工作流的可复用热缓存和验收记录；可用空间约34.8 GiB，未触及32 GB停线。本次用户已授权本地提交，未授权推送。
