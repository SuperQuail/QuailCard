---
type: fix
scope: agent
packages:
  quailcard: patch
---

Agent 写笔记与卡片时按应用支持的 LaTeX 写法输出公式：行内用 `$…$`，独占一行的块级公式用 `$$…$$`，避免生成渲染不出来的多行公式。
