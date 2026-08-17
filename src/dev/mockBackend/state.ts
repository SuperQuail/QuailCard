import type { ProviderSummary } from "../../domain/types";

/**
 * 浏览器演示环境的“内存数据库”。
 *
 * 说明与契约：
 * - 本文件只负责保存演示数据（笔记、卡片、供应商、各种 UI 偏好），不实现任何业务规则。
 * - 复习调度、AI 判定等业务规则一律不在本文件出现，见 scheduling.ts（且已简化为演示级）。
 * - 数据只存活于当前页面会话，刷新即丢失——这是有意为之，演示不应伪装成持久化。
 */

/** 演示用笔记：只保留正文与修改时间，用于支撑文件树与阅读视图。 */
export interface MockNote {
  content: string;
  mtime: number;
}

/** 演示用卡片：字段与领域类型 NoteCard 对应，但调度字段仅用于演示排序，不代表真实算法。 */
export interface MockCard {
  id: string;
  notePath: string;
  sourceRef: string;
  kind: "vocabulary" | "qa" | "ai";
  front: string;
  back: string;
  detail: string;
  example: string;
  aliases: string[];
  rubric: string[];
  position: number;
  schedulerPhase: string;
  dueAt: number;
  intervalDays: number;
  totalReviews: number;
  version: number;
}

/** 演示用供应商：与领域类型 ProviderSummary 保持一致形状，方便直接透传给 UI。 */
export type MockProvider = ProviderSummary;

/** 全局内存状态。集中存放，避免散落在多处难以追踪。 */
export const notes = new Map<string, MockNote>();
export const cards = new Map<string, MockCard>();
export let vaultPath: string | null = null;
export let fontSize: "compact" | "standard" | "comfortable" = "comfortable";
export let cardSequence = 0;
export let vaultProtection: "default" | "password" = "default";
export let vaultLocked = false;
export let aiGradingEnabled = false;
/** 演示用历史 Vault 路径，仅用于填充“最近打开”列表。 */
export const recentVaults: string[] = ["D:\QuailVault", "D:\Documents\MyKnowledge"];

/** 演示供应商列表，覆盖 API Key 与订阅两种形态，便于展示设置界面。 */
export const mockProviders: MockProvider[] = [
  { id: "openai", name: "OpenAI", shortCode: "OA", protocol: "OpenAI Compatible", model: "gpt-4.1-mini", baseUrl: "https://api.openai.com/v1", hasApiKey: true, hasCredential: true, authType: "api_key", oauthAccountId: null, providerType: "api", supportsVision: true, status: "connected" },
  { id: "anthropic", name: "Anthropic", shortCode: "AN", protocol: "Anthropic Messages", model: "claude-sonnet-4-5", baseUrl: "https://api.anthropic.com", hasApiKey: false, hasCredential: false, authType: null, oauthAccountId: null, providerType: "api", supportsVision: true, status: "untested" },
  { id: "opencode_go", name: "OpenCode Go", shortCode: "OG", protocol: "OpenAI Compatible", model: "deepseek-v4-flash", baseUrl: "https://opencode.ai/zen/go/v1", hasApiKey: false, hasCredential: false, authType: null, oauthAccountId: null, providerType: "api", supportsVision: false, status: "untested" },
  { id: "openai_subscription", name: "OpenAI 订阅", shortCode: "OS", protocol: "OpenAI Compatible", model: "gpt-5.5", baseUrl: "https://chatgpt.com/backend-api/codex/responses", hasApiKey: false, hasCredential: false, authType: null, oauthAccountId: null, providerType: "openai_subscription", supportsVision: true, status: "untested" },
];

/** 当前活动供应商 id，默认第一个。 */
export let mockActiveProviderId = "openai";

/**
 * 以下为可变标量的读写函数。
 *
 * 说明：ESM 禁止跨模块重新赋值导入的 `let`，因此对这些会被外部改写的
 * 标量提供显式 setter，保证状态修改都在本文件内部完成、可被追踪。
 */
export function setVaultPath(value: string | null): void {
  vaultPath = value;
}
export function setFontSize(value: "compact" | "standard" | "comfortable"): void {
  fontSize = value;
}
export function setVaultProtection(value: "default" | "password"): void {
  vaultProtection = value;
}
export function setVaultLocked(value: boolean): void {
  vaultLocked = value;
}
export function setAiGradingEnabled(value: boolean): void {
  aiGradingEnabled = value;
}
export function setActiveProviderId(value: string): void {
  mockActiveProviderId = value;
}
export function setCardSequence(value: number): void {
  cardSequence = value;
}

/**
 * 初始化演示数据。
 *
 * 契约：幂等——只要已存在笔记就不再重复填充，避免多次调用污染状态。
 * 种子内容仅为展示 UI 用，不宣称与真实 Vault 结构一致。
 */
export function ensureSeeded(): void {
  if (notes.size > 0) {
    return;
  }
  const now = Math.floor(Date.now() / 1000);
  const seedNotes: Array<[string, string]> = [
    [
      "英语/考研词汇 · 抽象概念.md",
      `# 考研词汇 · 抽象概念

本周整理的三个高频词，都与「变化、思考、恢复」相关。释义来自真题语境，例句可以直接拿来造句。

## ephemeral

词性 adj.，意思是短暂的、转瞬即逝的。常用来形容名声、现象与情绪。

Fame in the digital age can be remarkably ephemeral. ^qc-card-ephemeral

## contemplate

词性 v.，深入思考、仔细考虑。比 think 更正式，后面常接名词或动名词。

She paused to contemplate the consequences of her choice. ^qc-card-contemplate

## resilient

词性 adj.，有复原力的、能迅速恢复的。既可以形容人，也可以形容系统与经济。

A resilient system recovers without losing its structure. ^qc-card-resilient
`,
    ],
    [
      "计算机/Rust 所有权与借用.md",
      `# Rust 所有权与借用

所有权是 Rust 最核心的机制。理解它，才能理解为什么 Rust 不需要垃圾回收，也能保证内存安全。

## 所有权规则

每个值在任一时刻都只有一个所有者。所有者离开作用域时，值会被释放。

请解释：Rust 所有权为什么能避免悬垂引用？

## 借用规则

借用分为共享借用与可变借用。可变借用是独占的，这是防止数据竞争的关键。

可变借用为什么不能与其他借用同时存在？
`,
    ],
    [
      "认知科学/记忆与学习策略.md",
      `# 记忆与学习策略

认知心理学关于记忆的研究，直接指导了卡片复习的设计：为什么主动回忆比重复阅读有效。

工作记忆与长期记忆最核心的区别是什么？

什么是提取练习，它为什么有效？

- 间隔重复：按遗忘曲线安排复习时间
- 交错练习：混合不同类型的内容
- 生成效应：先自己作答再看答案
`,
    ],
    [
      "收件箱/灵感随手记.md",
      `# 灵感随手记

费曼学习法的核心：如果你不能把一个概念讲给一个完全不懂的人听，说明你自己还没有真正理解它。

刻意练习要求持续停留在学习区——任务难度略高于当前能力，同时能获得即时反馈。

人类大脑对图像的记忆远强于抽象文字，把知识画成结构图比做线性笔记更利于回忆。
`,
    ],
  ];
  for (const [path, content] of seedNotes) {
    notes.set(path, { content, mtime: now });
  }

  const seedCards: Array<[string, MockCard]> = [
    ["card-ephemeral", makeCard("英语/考研词汇 · 抽象概念.md", "vocabulary", "短暂的；转瞬即逝的", "ephemeral", "/ɪˈfemərəl/", "Fame in the digital age can be remarkably ephemeral.", 0)],
    ["card-contemplate", makeCard("英语/考研词汇 · 抽象概念.md", "vocabulary", "深入思考；仔细考虑", "contemplate", "/ˈkɑːntəmpleɪt/", "She paused to contemplate the consequences of her choice.", 1)],
    ["card-resilient", makeCard("英语/考研词汇 · 抽象概念.md", "vocabulary", "有复原力的；能迅速恢复的", "resilient", "/rɪˈzɪliənt/", "A resilient system recovers without losing its structure.", 2)],
    ["card-ownership", makeCard("计算机/Rust 所有权与借用.md", "qa", "请用自己的话解释：Rust 所有权为什么能避免悬垂引用？", "值在任一时刻只有一个所有者；所有者离开作用域时值被释放，借用检查器会阻止引用活得比所有者更久。", "来源：2.1 所有权规则", "", 0)],
    ["card-borrow", makeCard("计算机/Rust 所有权与借用.md", "qa", "可变借用为什么不能与其他借用同时存在？", "独占可变借用可以避免读写冲突和数据竞争，保证修改期间没有其他引用访问该值。", "来源：2.3 借用规则", "", 1)],
    ["card-working-memory", makeCard("认知科学/记忆与学习策略.md", "qa", "工作记忆与长期记忆最核心的区别是什么？", "工作记忆容量有限，用于短时间保持和操作当前信息；长期记忆容量更大，负责持久保存知识与经验。", "来源：4.2 记忆系统", "", 0)],
    ["card-retrieval", makeCard("认知科学/记忆与学习策略.md", "qa", "什么是提取练习，它为什么有效？", "提取练习要求学习者主动从记忆中召回信息。主动提取会强化检索路径，比重复阅读更能提高长期保持率。", "来源：4.5 学习策略", "", 1)],
  ];
  for (const [id, card] of seedCards) {
    card.sourceRef = `^qc-${id}`;
    // 仅演示：让少数卡片到期，方便直接进入复习演示，不代表真实调度结果。
    if (id === "card-resilient" || id === "card-borrow") {
      card.dueAt = now + 86_400 * 2;
      card.schedulerPhase = "review";
      card.intervalDays = 2;
    }
    cards.set(id, card);
  }
  cardSequence = seedCards.length;
}

/**
 * 构造一张演示卡片。
 *
 * 契约：调用方只传入展示必需字段，调度相关字段统一由演示默认值填充。
 */
export function makeCard(
  notePath: string,
  kind: MockCard["kind"],
  front: string,
  back: string,
  detail: string,
  example: string,
  position: number,
): MockCard {
  const now = Math.floor(Date.now() / 1000);
  return {
    id: `card-${Date.now()}-${position}`,
    notePath,
    sourceRef: "",
    kind,
    front,
    back,
    detail,
    example,
    aliases: [],
    // 仅演示：问答卡预置两个示意评分要点，方便展示 AI 判定界面。
    rubric: kind === "qa" ? ["关键要点一", "关键要点二"] : [],
    position,
    schedulerPhase: "new",
    dueAt: now,
    intervalDays: 0,
    totalReviews: 0,
    version: 0,
  };
}
