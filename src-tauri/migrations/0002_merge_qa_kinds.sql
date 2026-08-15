-- 合并卡片类型：AI 问答并入普通问答，判定方式改为全局设置控制。
UPDATE cards SET kind = 'qa' WHERE kind = 'ai';
