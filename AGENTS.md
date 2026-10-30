# AGENTS.md

## 项目速览

- 产品：本地优先的 Markdown 笔记 + 记忆卡片桌面应用。
- 技术栈：Rust / Tauri 2 / Vue 3 / TypeScript / 文件存储（TOML 配置 + JSON 数据）/ Tailwind CSS 4。
- 核心功能：
  - 用户提供笔记、图片、文本，AI 转换为记忆卡片格式。
  - 格式一：单词记忆卡，必须包含听写模块，根据释义默写。
  - 格式二：问答卡，根据笔记生成一问一答。
  - 扩展：AI 判定不是新卡片格式，而是问答卡的评分方式。
- API：支持 OpenAI 与 Claude 格式，支持自定义 BaseURL 和类 OpenCode 的供应商选择。
- 数据事实：笔记是真实 `.md` 文件；卡片与其调度状态、复习历史存于 Vault 内 `.quailcard/**/*.json`（每篇笔记一个文件）；供应商配置（providers.toml）、加密凭据密文（vault.bin）、全局设置（settings.toml）存于应用数据目录；ECDICT 词典为随应用打包的只读资源（rusqlite 访问）；前端不直接读写文件系统。
- 双链与反链（强制）：`[[目标]]` 的解析规则由 **`src/domain/linkResolution.fixture.json`** 与 `storage/links.rs` 共同锁定，前端 `domain/links.ts` 与后端 `links::resolve` 必须给出完全相同的结果——改规则先改夹具，两端测试会同时失败。偏移一律用 **UTF-16**（与卡片 `CardSource` 同坐标系）。改名/移动时若设置开启（默认开），后端按"先规划后改名"改写引用方正文，并同步平移卡片来源锚点；与改写区间相交的锚点保持原样而不猜测位置。
- frontmatter（强制）：只读，应用**永不写回** YAML 头；解析失败按"无元数据"处理，绝不阻断 Vault 扫描。标签 = frontmatter 声明（在前）+ 正文行内标签，合并去重后限量；别名参与双链解析与补全。
- 图片附件动作（强制）：编辑器里图片的**复制 / 下载 / 在文件夹中定位**一律由后端解析引用（`vaultfs::attachments::attachment_file_path`）并校验后再执行，前端只提交 `(notePath, source)`，**不提交任何文件路径**。另存目标只能来自系统另存对话框，写入必须"同目录临时文件 + rename"替换，禁止半截文件；单个附件上限 10 MiB（`MAX_IMAGE_BYTES`）对导入、读取、复制、另存统一生效。剪贴板写入走 `image_clipboard`（解码 → RGBA → arboard），失败只回 `CLIPBOARD_ERROR`/`SAVE_ERROR`/`IMAGE_DECODE_ERROR` 等安全错误码。
- 删除语义（强制）：删除默认走**系统回收站**（`vaultfs::remove` 的 `Remover` 策略），只有用户在设置里显式开启「永久删除」才直删。回收站模式下笔记的卡片文件必须**移到 `.quailcard/.detached/`** 而不是删除，使用户从回收站还原笔记后卡片与复习历史能自动接回（接回发生在卡片加载前，见 `storage/card_detach.rs`）。隔离区不属于 active 数据：加载跳过、损坏跳过、超过 30 天的孤儿在打开 Vault 时清理。

## 目录与分层边界

- `src-tauri/src/commands.rs`：只做参数接收、状态注入和委派；不写 SQL、HTTP、业务规则。
- `src-tauri/src/storage/`：唯一允许应用数据持久化的层；TOML 配置 + JSON 数据文件，按聚合拆存储域（settings / providers / vault_file / cards / review / search / notes）。
- `src-tauri/src/scheduler.rs`：纯领域调度算法，不依赖存储实现或 Tauri。
- `src-tauri/src/vault_crypto.rs`：纯密码学原语与载荷编解码；保险库服务只做会话与编排。
- `src-tauri/src/services/`：用例编排；依赖接口，不直接依赖存储与网络实现。
- `src-tauri/src/ai/`：供应商协议适配。`llm/` 是供应商中立调用层（词汇、chunk、assembler、failure、adapter 契约、runtime），`adapters/` 每个协议一个 adapter，`tools/` 是工具声明与参数校验，`generation/`、`evaluation/` 各自持有提示词、profile 与工具集。
- `src/api/`：只封装 Tauri invoke。
- `src/domain/`：类型与纯函数。
- `src/services/`：前端用例状态；组件不得直接持有全量状态。
- `src/components/`：展示与事件；不直接调用后端。
- `references/`、`dist/`、`prototype-dist/`、`target/` 不进入版本控制，不参与构建。

## 设计原则：SOLID（强制）

### S 单一职责

- 一个文件、模块或组件只有一个变化理由。
- 修改已有文件时，若该文件已超过 1000 行，必须顺带拆分；暂时不能拆的，在本文件“当前技术债”登记，并禁止继续膨胀。
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
- Rust 用例不接收整个 `Storage`，只接收所需存储域方法或 trait。

### D 依赖倒置

- 领域逻辑（复习调度、AI 判定流程、路径安全）依赖接口，不依赖存储实现、reqwest、invoke。
- 前端只依赖 `api/` 抽象；后端 service 只依赖 repository / port trait。

## 编码与安全

- 每个函数都有中文注释；注释说明原因与契约，不重复代码事实。
- 文件路径必须经 `vaultfs` 净化；禁止绕过沙箱直接操作文件系统。
- 密码、API Key、OAuth token 必须 zeroize；禁止进入日志、错误消息、前端 DTO。
- 返回前端的错误只保留 `code + 安全 message`；原始 rusqlite / reqwest / 文件解析错误只写后端日志。
- 文件格式演化四律（防迁移，详见 storage 模块文档）：读容忍（缺字段取默认、未知字段忽略、禁 deny_unknown_fields）；写完整（每次保存写出当前格式，废弃字段自清洁）；只增不改（永不改名/删除/改类型已有字段）；版本熔断（文件 formatVersion 高于当前应用时拒绝打开）。
- 损坏策略：配置文件损坏时备份为 `.corrupt-<时间戳>` 后重建默认；卡片与保险库文件损坏直接报错拒开，绝不自动重置。所有写入走临时文件 + rename 原子替换。
- Rust `models.rs` 与前端 `src/domain/types.ts` 是同一 wire 契约，必须同提交同步，字段统一 camelCase。
- 前端 `mock.ts` 仅用于浏览器演示，不得复制或替代后端业务规则。
- 避免不必要的抽象：抽象出现第二个真实消费者时再引入，安全与分层边界除外。

## UI

- UI 以护眼、长时间使用为基础，不追求美观，但必须好用；常用按钮不放到太深的位置。
- 沿用现有样式与组件分层；界面改动需补充组件测试，避免把业务状态下沉到展示组件。

## 提交规范（强制）

- 格式：`<type>(<scope>): <祈使句>`。`type` 用 `feat` / `fix` / `perf` / `refactor` / `docs` / `test` / `build` / `ci` / `chore` / `revert`，`scope` 用受影响的模块名（`links`、`vault`、`app` 等）。
- 标题必须是**动词开头的祈使句**，一句话回答“这个提交做了什么”：`支持双链与反链`、`改用回收站删除`、`增加图片右键操作`。
- 禁止名词罗列与修饰语堆叠（如 `双链、反链与改名联动`）；标题不超过 50 字符，不以句号结尾。
- 原因、影响与取舍写正文，每行不超过 72 字符；正文只解释“为什么”，不复述 diff。
- 一个提交只做一件事；跨主题的改动拆成多个提交，共享契约（`models.rs` / `src/domain/types.ts`）随首个使用者一起提交。
- 用户可见的改动必须配一条 `.changes/<slug>.md` 记录（格式见 `.changes/README.md`）；只做内部接线或依赖升级时不加记录。
- 提交信息用中文，`type` / `scope` 用英文。

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
