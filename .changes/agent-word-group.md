---
type: feat
scope: agent
packages:
  quailcard: minor
---

Agent 现在可以创建与更新单词组（`.qword`）：提交结构化词表即可写入，页 id 由后端生成，保存后立即生成单词卡片；更新按单词沿用原页与复习历史，未出现在新词表里的页会连复习记录一起删除。
