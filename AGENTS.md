# AGENTS.md

## 项目速览

- 产品：本地优先的 Markdown 笔记 + 记忆卡片桌面应用。
- 技术栈：Rust / Tauri 2 / Vue 3 / TypeScript / SQLite（sqlx）/ Tailwind CSS 4。
- 核心功能：
  - 用户提供笔记、图片、文本，AI 转换为记忆卡片格式。
  - 格式一：单词记忆卡，必须包含听写模块，根据释义默写。
  - 格式二：问答卡，根据笔记生成一问一答。
  - 扩展：AI 判定不是新卡片格式，而是问答卡的评分方式。
- API：支持 OpenAI 与 Claude 格式，支持自定义 BaseURL 和类 OpenCode 的供应商选择。
- 数据事实：笔记是真实 `.md` 文件；卡片、复习调度、供应商凭据保存在本机 SQLite；前端不直接读写文件系统。

## 目录与分层边界

- `src-tauri/src/commands.rs`：只做参数接收、状态注入和委派；不写 SQL、HTTP、业务规则。
- `src-tauri/src/database/`：唯一允许出现 SQL 的层；按聚合拆 repository。
- `src-tauri/src/scheduler.rs`：纯领域调度算法，不依赖 SQLite 或 Tauri。
- `src-tauri/src/vault_crypto.rs`：纯密码学原语与载荷编解码；保险库服务只做会话与编排。
- `src-tauri/src/services/`：用例编排；依赖接口，不直接依赖存储与网络实现。
- `src-tauri/src/ai/`：供应商协议适配；每个协议一个 adapter。
- `src/api/`：只封装 Tauri invoke。
- `src/domain/`：类型与纯函数。
- `src/services/`：前端用例状态；组件不得直接持有全量状态。
- `src/components/`：展示与事件；不直接调用后端。
- `references/`、`dist/`、`prototype-dist/`、`target/` 不进入版本控制，不参与构建。

## 设计原则：SOLID（强制）

### S 单一职责

- 一个文件、模块或组件只有一个变化理由。
- 新文件上限：Rust ≤ 400 行；TS / Vue SFC ≤ 300 行。
- 修改已有文件时，若该文件已超过 500 行，必须顺带拆分；暂时不能拆的，在本文件“当前技术债”登记，并禁止继续膨胀。
- 写完功能必须扫描代码行数，高于合理预期就拆分。

### O 开闭原则

- 新增供应商协议、认证方式、卡片类型、命令动作时，新增 adapter / registry / strategy / 子组件。
- 禁止继续在 `protocol`、`auth_type`、`kind`、`command` 上堆 `match` / `switch` / `if-else` 特判。

### L 里氏替换

- 所有 trait / interface 实现必须可互相替换，并通过同一套契约测试。
- 禁止用 `provider_type`、`kind` 等字符串判断给某些实现开后门。

### I 接口隔离

- Vue 组件只接收它需要的 props / emits；禁止组件深处直接消费全量 `useAppState()`。
- 前端 store 按 vault / note / card / review / provider / ui 拆域。
- Rust 用例不接收整个 `Database`，只接收所需 repository trait。

### D 依赖倒置

- 领域逻辑（复习调度、AI 判定流程、路径安全）依赖接口，不依赖 SQLite、reqwest、invoke。
- 前端只依赖 `api/` 抽象；后端 service 只依赖 repository / port trait。

## 编码与安全

- 每个函数都有中文注释；注释说明原因与契约，不重复代码事实。
- 文件路径必须经 `vaultfs` 净化；禁止绕过沙箱直接操作文件系统。
- 密码、API Key、OAuth token 必须 zeroize；禁止进入日志、错误消息、前端 DTO。
- 返回前端的错误只保留 `code + 安全 message`；原始 SQLx / reqwest 错误只写后端日志。
- SQL 一律参数绑定；schema 变更只能通过新 migration。
- 禁止静默破坏性迁移；删除旧库、旧表前必须显式备份。
- Rust `models.rs` 与前端 `src/domain/types.ts` 是同一 wire 契约，必须同提交同步，字段统一 camelCase。
- 前端 `mock.ts` 仅用于浏览器演示，不得复制或替代后端业务规则。
- 避免不必要的抽象：抽象出现第二个真实消费者时再引入，安全与分层边界除外。

## UI

- UI 以护眼、长时间使用为基础，不追求美观，但必须好用；常用按钮不放到太深的位置。
- 生成具体 UI 前，先用 twcss 生成原型，用户确认后再继续。
- 交互原型放在 `prototype/`，生成物不得提交。

## 提交前检查

```sh
pnpm typecheck
pnpm test
pnpm build
cd src-tauri && cargo fmt --all -- --check
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
cd src-tauri && cargo test
git diff --cached --check
```
