# 项目技能

本目录是 OpenRust 可复用工作流的唯一维护入口。通用协作与授权规则见 [AGENTS.md](../../AGENTS.md)，项目事实在 `.agents/project/`。

每个技能使用 `<name>/SKILL.md` 并带 YAML `name` / `description`。Codex 从当前目录向 Git 根目录扫描本目录，先加载名称与描述，匹配任务后再读正文；其他 Agent 通过 `AGENTS.md` §3 路由按需读取。

- [implement-crate](implement-crate/SKILL.md)：按已定 contract 实现或扩展一个 `openrust-*` crate。

后续按需补充：项目模板维护、生成器快照测试、双库矩阵排错。
