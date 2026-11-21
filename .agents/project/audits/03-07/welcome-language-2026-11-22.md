# 欢迎页浏览器语言识别（2026-11-22）

## 实现范围

欢迎页按 Accept-Language 的 q 权重选择 en／zh_CN／zh_TW；权重相同时保持浏览器顺序，q=0 条目不参与选择，未知或非法条目跳过，无匹配时使用受支持的 APP_LOCALE，否则英文。英文地区变体归并英文，中文 Hans／Hant 优先地区，TW／HK／MO 为繁体、CN／SG／裸 zh 为简体；仅欢迎页自动选择，不改变全站默认语言。

WelcomeController 复用 canonical_locale（已有 ICU 规范化）及 Lang::getWith 的显式 locale 参数；一次性准备翻译数据，模板保持同一份，不引入管理器、中间件、依赖或共享语言状态。语言文件为 lang/{en,zh_CN,zh_TW}/welcome.json；八项文案与 html lang、Content-Language、Vary、文档和部署链接同步，名称／版本与原布局保留。

Docker runtime 增加 COPY lang/，三语部署命令增加复制 lang；避免二进制与模板部署后缺少翻译。三语国际化正文说明欢迎页的支持与回退规则。

## 调研与适配

已联网保存并阅读固定 Laravel 文档 960eedf0911155ad16326ade49c0ea5d18eda0b8 的 localization.md，以及 Symfony HttpFoundation v7.3.0 Request.php 的 getPreferredLanguage／getLanguages。前者用于语言文件与 fallback 组织，后者用于请求头优先级及语言变体协商思路。现有 OpenRust Request 没有 getPreferredLanguage，媒体类型 Accept 解析也不适用于语言范围，故在欢迎页控制器增加私有选择函数，未新增公开框架 API，也不宣称完整 Symfony 语言协商等价。HTTP qvalue 用整数千分位按标准语法处理，非法／重复参数忽略该条目；没有可选择项时仍返回默认欢迎页，不返回 406。

来源：
- https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md
- https://github.com/symfony/http-foundation/blob/v7.3.0/Request.php

## 验收

基线 fb5240e1ae08855857d9b9988ac5453d8623e608，保留此前未提交修改。全部临时源码、日志、独占 Cargo target 与 TMPDIR 位于 target/tmp/welcome-locale-20261122/；Rust 1.94.0，正常并发、保留增量，dev/test debug=0、strip=symbols，非优化 release 构建。最终相关输入逐字节核对一致。

| 范围 | 结果与证据 |
| --- | --- |
| 真实欢迎页 HTTP Kernel | tests/welcome.rs 四项通过：三语／地区／文字体系／优先级／重复请求头、无效和未知语言／q=0／默认回退、并发请求与独立应用隔离、缺键英文 fallback；welcome-tests.log |
| 原欢迎页入口 | tests/startup.rs 的具名视图查找／替换／缺视图行为回归通过；startup-welcome.log |
| 独立部署 | --features standalone 构建通过；二进制、resources、lang、public 复制到隔离部署目录，以空 PATH、异地 cwd 启动真实 HTTP，无开发库操作；standalone-build.log、http-server.log |
| 实际浏览器 | Chromium 12 组合：三语 × 桌面／手机 × 明／暗；文案、文档／部署链接、html lang、响应头、无横向溢出与页面异常通过；browser-complete.log、browser-results.json；截图已人工检查简体手机亮色与繁体桌面暗色 |
| 质量检查 | 受影响根应用 bin、welcome／startup 测试严格 Clippy 通过；全 workspace fmt --check 与 git diff --check 通过；clippy.log |
| 文档 | 三语站点构建（含内容检查）与全导航支持边界浏览器检查通过；docs-final-results.json、docs-build-final.log、docs-scope-final.log |

初次测试夹具误传 &PathBuf 而 configure 接收拥有所有权的 PathBuf，已改 clone 并实跑通过；初次文档 scope 缺少浏览器环境变量已补全；临时浏览器夹具误把 main 在桌面／手机的原布局都断言为同一 display，已按实际断点分别检查 grid／flex 并完成全部 12 组合。初次失败日志保留，不作为产品缺陷或成功证据。

## 边界与收尾

未重建 Docker 发布镜像、未运行 Windows 或优化 release 构建；本次验证为 macOS ARM64 的真实本地服务与独立部署目录，不能替代全部部署平台验收。浏览器与本批应用进程退出，未启停原开发容器、未访问开发数据库或删除卷。源码、日志和热缓存保留。2026-11-22 用户授权修正文档语言标签与空行、复核后本地提交本轮改动，不包含推送。

提交前复核：三语国际化正文的地区缩写已改为完整 zh-* 语言标签，并移除多余空行。当前内容检查、隔离文档重新构建、三语全导航浏览器与 fmt 检查通过，暂存差异无空白错误；日志 precommit-results.json 与 precommit-*.log。全部 3,246 项受跟踪输入与隔离快照内容一致（artisan.bat 仅行尾归一化），新增欢迎页语言文件和测试另行逐字节核对一致。
