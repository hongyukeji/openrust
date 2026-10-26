# 项目技能

本目录是 OpenRust 可复用工作流的唯一维护入口。通用协作与授权规则见 [AGENTS.md](../../AGENTS.md)，项目事实在 `.agents/project/`。

每个技能使用 `<name>/SKILL.md` 并带 YAML `name` / `description`。Codex 从当前目录向 Git 根目录扫描本目录，先加载名称与描述，匹配任务后再读正文；其他 Agent 通过 `AGENTS.md` §3 路由按需读取。

- [implement-crate](implement-crate/SKILL.md)：按已定 contract 实现或扩展一个 `openrust-*` crate。
- [codebase-design](codebase-design/SKILL.md)：设计模块接口与清晰 seam；项目内术语和 ADR 优先于技能通用词汇。
- [diagnosing-bugs](diagnosing-bugs/SKILL.md)：对复杂故障和性能回归按证据逐步定位；遵守项目脱敏与验收规则。
- [tdd](tdd/SKILL.md)：按可观察行为做 red-green 测试循环；具体测试范围遵循用户要求和仓库门禁。

以上三个技能改编自 [mattpocock/skills](https://github.com/mattpocock/skills)（MIT）；保留其 `SKILL.md` 的来源与许可声明，并补充了 OpenRust 的优先规则。

Codex 会依据每个 `SKILL.md` 的 `description` 自动匹配相关任务并按需读取正文，不会在每个请求中无差别加载。项目 `AGENTS.md`、ADR、当前计划与用户指示优先于这些通用技能；已存在的技能说明若与它们冲突，以项目规则为准。

后续按需补充：项目模板维护、生成器快照测试、双库矩阵排错。
