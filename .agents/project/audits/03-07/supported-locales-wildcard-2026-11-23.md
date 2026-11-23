# 语言候选配置收口

## 用户决定

2026-11-23 用户确认仅保留 `*` 表达自动发现全部可用语言；语言列表限制候选，空值关闭语言协商。该决定覆盖同日较早的显式 `null`／`(null)` 自动发现约定。

- 环境变量未设置时使用代码默认值 `*`。
- `APP_SUPPORTED_LOCALES=*` 发现框架及应用的可用语言资源。
- 逗号分隔的语言列表限定候选；空环境值生成空数组并关闭语言协商。
- 显式环境值 `null`／`(null)`（不区分大小写）返回配置错误。
- 框架直接配置接受 `"*"` 或语言标签数组，拒绝显式 JSON `null`。缺键仍采用默认发现；显式中间件候选保留最高优先级。
- `.env` 与 `.env.example` 原有三语列表保持不变；其他配置项已有的 null 语义不受影响。

复用既有解析器、SetLocale 与 Loader，不增加配置项、兼容分支或新的语言发现机制。三语本地化说明、现有独立HTTP验收脚本和项目规范同步更新；历史验收记录保留原时点结论。

## 验收证据

源码快照及日志：`target/tmp/welcome-documentation-links-20261123/`；Cargo target 复用 `target/tmp/model-fields-sync-20261123/cargo-target`。全部 Cargo 通过项目 `tests/build_guard.py` 启动，使用 Rust 1.94.0 和项目内 TMPDIR。

- `supported-locales-root.log`：配置测试3项、欢迎页HTTP测试36项通过。覆盖缺省、星号、显式列表、空值停用及四种大小写 null 写法拒绝。
- `supported-locales-framework-resumed.log`：框架13项通过，覆盖直接JSON null拒绝、配置修复后重新启动、星号发现、列表和显式中间件优先级。合计52项通过、0忽略。
- `supported-locales-accepted-inputs.json`：五份受影响Rust源码和测试与通过验收的快照逐文件一致。
- Python HTTP脚本通过AST语法检查；本轮未启动其独立服务器流程，相关配置和HTTP行为由上述消费者测试验证。
- 本批路径 `git diff --check` 通过。

首次框架验收编译触及32 GB停线，守卫安全退出（75）并清理编译profile后重试；中断不计通过。守卫事件记录实际释放74,379,767,808字节，锁文件inode保留；源码、验收日志、数据库及数据卷未清理。最终可用空间约87.2 GiB。未启动独立服务或Docker资源，保留本工作流可复用缓存与验收记录。

用户随后授权将本单元单独本地提交，未授权推送；其他协作者的改动由其独立收口。

## 独立提交组合

提交前以已提交HEAD构建仅包含本单元10个路径的独立快照，不含十八语默认值加载、其他词典或品牌清理。`supported-commit-root.log`：配置3项、欢迎页HTTP36项通过；`supported-commit-framework.log`：框架12项通过，共51项通过、0忽略。完整在途组合的额外十八语用例仍由前述52项记录覆盖，不混算。

独立提交保留已提交的私有 `supported_locales(env)` 名称，将星号默认值和严格null拒绝接入；工作区其他协作者的 `get_supported_locales(env, key, default)` 包装保持不动，由所属批次继续提交。两种组合均按同一公开配置行为验收。未跟踪的十八语独立HTTP脚本仍留给所属批次，未夹带整份文件。
