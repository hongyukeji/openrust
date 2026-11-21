# 当前交接

## 当前范围

2026-11-21 按用户要求独立完成发布前的本地收尾，不再联系其他对话或委派子代理。真实服务目前不具备条件，统一保留在 [todo.md](todo.md)，不算验收通过，也不阻塞本轮可执行的本地工作。

根应用继续使用 `bootstrap/packages.rs::manifest()` 装配包清单，默认 Provider 不预装扩展。开发与 Docker 共用根 `.env`；`docker/.env.example` 仅提供可选 Docker 字段参考，Compose 不加载 `docker/.env`。注释与未国际化提示优先简体中文。

## 本轮已确认

- 根 17 个定向测试目标共 69 项通过；六扩展组合 110 项通过，11 个 Redis 忽略项已用隔离 Redis 8.10.3 补跑。
- PostgreSQL 18.7、MySQL 8.4.13、SQLite 与 Redis／database／file 缓存的定向组合通过；缺连接提前返回的测试不计为数据库验证。
- Telescope 初始化采集边界与终止写入修复；数据库缓存过期测试改为符合整秒存储契约的有限轮询；生成应用夹具只过滤根运行目录，保留包内同名源码。
- Telescope／Horizon／Pulse 的真实本地服务、桌面和手机浏览器通过；Linux ARM64 Debian Bookworm、Rust 1.94.0 的四个选定目标通过。Windows 原生构建受 Parallels 共享文件系统 `os error 87` 阻塞，条件和后续矩阵已列待办。
- 全 workspace／all-targets／all-features 严格 Clippy、格式通过；全 feature 锁定依赖公告 0 errors／0 warnings，672 项许可证来源均有声明。实际发行许可义务仍按交付物复核。

## 本轮完成

修复已本地提交为 `0855b54f2a984ac25b68ab74d4e2ca83962d68fc`，验收快照代码／测试与该提交独立归档逐字节一致。PostgreSQL／MySQL完整生成应用、重复初始化与资源安装、实际HTTP及独立开发profile二进制搬迁运行通过；未执行优化release构建。

19库可追溯API已从该提交新归档导出，覆盖2,171项输入指纹，产物在 `target/tmp/final-acceptance-20261121/api-export/`；三语联合站点在同批 `api-source/docs/.vitepress/dist/`。最终结果与失败尝试边界统一见 [08-04](audits/08-04/README.md#2026-11-21-发布前本地收尾)。19库／2,171项输入API导出、三语联合构建、14项过期／非法输入负例、7项CLI拒绝、API与普通页面各12浏览器组合及6组全站导航全部通过。最后可用约59.29 GB／55.22 GiB。本地收尾不等于正式发布，真实目标与Windows矩阵仍见待办；其他对话的未跟踪Laravel对照审计保留，未纳入本轮提交和全部API等价结论。

## 产物与证据

本轮全部临时源码、编译目录、日志和浏览器材料均在项目 `target/tmp/final-acceptance-20261121/`；子进程使用项目内 TMPDIR 与独占 CARGO_TARGET_DIR。已安全回收本批闲置Linux编译缓存7,486,048 KiB，保留源码、日志和锁inode；本批容器已回收，Windows恢复初始suspended。此前清除构建产物时旧 `target/tmp/` 内容一并被移除，旧审计中记录的路径不能当作仍现存的证据；本轮重新生成必要材料。不得整目录删除根 target；32 GB 停线按 [构建产物管理](conventions.md#构建产物管理)安全清理。

## Git 与发布授权

保留用户与其他协作者改动，包括本轮出现的未跟踪审计报告。2026-11-11 用户授权仍有效：阶段实现、验收、三语说明和记录完成后可核定范围本地提交。未授权推送、改发布版本、打 tag、创建 Release 或生产部署。最终发布批准与真实服务验收继续单列。
