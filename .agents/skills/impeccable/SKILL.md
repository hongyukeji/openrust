---
name: impeccable
description: 设计、评审或打磨 OpenRust 应用的后台与移动界面，处理信息层级、布局、主题、文案和异常状态。适用于明确的 UI 任务，不用于后端实现或决定是否启动 V2。
license: Apache-2.0; see LICENSE
metadata:
  upstream: https://github.com/pbakaus/impeccable
  revision: 3671d50f0ac9a662ce0615e85a256110c9cd720b
  path: .agents/skills/impeccable
  adaptation: OpenRust admin and mobile design guidance; no upstream CLI or extension
---

# 应用界面设计

本地适配版，修改了上游流程与触发范围。只提供设计方法，不包含上游 CLI、浏览器扩展、自动 hooks 或完整命令集。许可见 [LICENSE](LICENSE)，上游声明见 [NOTICE.md](NOTICE.md)。

## 先定位任务

从当前页面、组件、设计 token 与 [V2 计划](../../project/plans/v2-admin-platform.md) 确认用户、主要操作、设备和状态。沿用根 `AGENTS.md`；产品事实维护在现有项目文档，设计事实放对应应用文档，不另建根目录 PRODUCT.md/DESIGN.md。

- 新页面：先确定主任务、信息层级、导航与失败恢复，再实现视觉。
- 改进现有界面：先看实际渲染与完整操作路径，保持已定身份和行为；局部打磨不隐含全站换肤。
- 只读评审：输出带截图或源码位置的发现、影响与建议，不自动改码。
- Admin 默认以操作效率、数据可读性和一致性为主；App 按移动场景重新组织任务，不机械缩小桌面表格。PC 端仅规划，不借设计任务启动实现。

## 设计与实现

涉及布局、表格、表单或多状态页面时按需读 [应用设计规则](references/application-design.md)。优先使用已选组件库和已有 token，避免复制一套按钮、对话框、主题系统。

比较主操作的可发现性、状态的可信度、页面密度及恢复成本；不用组件数、动画量或装饰数量评价设计。小修直接完成，有实质取舍才比较方案。

错误、空数据、加载、无权限、长文本与本地化都是设计输入。确认实际后端保证后再写界面文案，不把“请求已接收”显示成“任务已完成”，不把禁用按钮当成服务端授权。

## 验证

查看目标视口、缩放与实际内容下的渲染，完成关键操作再评价效果。Web 的语义、焦点和交互审查使用 [web-design-guidelines](../web-design-guidelines/SKILL.md)；需要功能验收时使用 [webapp-testing](../webapp-testing/SKILL.md)。App 使用 [uniapp-development](../uniapp-development/SKILL.md) 的平台验收边界。

不能运行页面时明确只完成源码/设计审查。截图证明视觉状态，不能替代行为测试；不要把技能安装视为 UI 已实现或验收通过。
