# 变更记录

每个用户可见的改动都需要在本目录新增一个 Markdown 文件。

记录示例：

    ---
    type: feat
    scope: cache
    packages:
      my-package: minor
    ---

    用一两句话描述这次改动。

字段说明：

- type: feat、fix、perf、docs、refactor、test、build、ci、chore、style 或 revert
- scope: 可选，影响范围
- packages: 包名与升级级别（patch、minor、major）
- release: 可选，默认 true；为 false 时只写入 Changelog

用 qegg commit 生成记录。
