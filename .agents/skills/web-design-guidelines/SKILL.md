---
name: web-design-guidelines
description: 审查 Vue Web 管理界面的可访问性、表单、焦点、响应式、国际化与交互质量。适用于 UI review 和上线前界面检查；不以浏览器规则代替 uni-app 原生端验收。
license: MIT; see LICENSE
metadata:
  upstream: https://github.com/vercel-labs/web-interface-guidelines
  revision: 434b7f91364665f2f733b310ec54809bf8f37937
  path: command.md
  adaptation: Locally maintained framework-neutral review checklist
---

# Web 界面审查

根据 Vercel 的 MIT 规则文件改编；本入口由项目编写，未复制 `agent-skills` 仓库的入口。规则固定在本地，日常审查无需在线拉取最新版。许可见 [LICENSE](LICENSE)。

1. 从任务定位页面、组件和目标状态，读取对应源码与当前组件库约定。仅查看相关范围，不扫描全站来完成一个局部审查。
2. 使用 [审查清单](references/review-checklist.md) 的相关部分；项目技术栈、设计 token 和语言规则优先于上游风格偏好。
3. 有可运行界面时验证键盘、焦点、窄屏、缩放及关键错误状态；只有源码证据时明确动态项未验证。
4. 输出按严重度排序的 `文件:行号 / 路由与状态 → 用户影响 → 修正建议 → 验证证据`。评审任务保持只读；修复任务在改后复验。

原生按钮和链接优先使用浏览器内建键盘行为，不机械加键盘监听导致重复触发。React 专属语法、固定虚拟化条数、英文标题大小写和装饰偏好不作为 Vue 后台硬性规则。

发现业务行为或权限问题时以 [webapp-testing](../webapp-testing/SKILL.md) 验证；样式检查通过不能证明认证、授权或端到端流程正确。
