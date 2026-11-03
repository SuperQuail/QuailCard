/**
 * 单词组文件（.qword）的前端领域模型。
 *
 * 契约（与后端 src-tauri/src/word_group.rs 对齐）：
 * - JSON、UTF-8、LF；formatVersion 当前 1，缺失按 1，高于 1 拒绝（版本熔断）。
 * - words 顺序即页面顺序，允许空数组与草稿页；页面 id 是卡片身份。
 * - 读容忍：缺字段取空串、未知字段忽略；写完整：稳定字段序、2 空格缩进、末尾换行。
 * 前端只做 JSON / 版本 / 基本形状校验；字段上限与 id 唯一性由后端保存时兜底，
 * 所以这里校验失败只把编辑器切成只读，绝不猜测内容后写盘。
 */
export const WORD_GROUP_FORMAT_VERSION = 1;

/** 单词组页数上限：与后端一致，添加页时先行截断，避免写盘才被拒绝。 */
export const WORD_GROUP_MAX_PAGES = 5000;

/** 单页字段上限：与后端一致，直接用作输入框 maxlength。 */
export const WORD_ENTRY_LIMITS = {
  word: 200,
  phonetic: 200,
  meaning: 2000,
  example: 8000,
} as const;

/** 一页一个单词：字段名与 ID 就是卡片契约，不得改名。 */
export interface WordEntry {
  /** 1–64 个 ASCII 字母/数字/-/_，全局唯一；即物化卡片的 id。 */
  id: string;
  word: string;
  /** 音标，通常形如 /ɪˈfemərəl/；空串表示未填。 */
  phonetic: string;
  meaning: string;
  example: string;
}

/** 单词组整份文档。 */
export interface WordGroupDocument {
  formatVersion: number;
  words: WordEntry[];
}

/** 解析结果：失败时给出可直接展示的中文原因，由编辑器渲染只读横幅。 */
export type WordGroupParseResult =
  | { ok: true; document: WordGroupDocument }
  | { ok: false; error: string };

/** 页面 id 合法形式：与后端校验完全一致。 */
const ENTRY_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

/** 把未知字段读成文本：缺失按空串；存在但不是字符串视为基本形状错误。 */
function readText(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  if (value === undefined || value === null) return "";
  return typeof value === "string" ? value : null;
}

/** 生成合法 id：优先 crypto.randomUUID，随机源不可用时退化为时间戳 + 随机十六进制。 */
export function createEntryId(): string {
  const cryptoApi = typeof globalThis.crypto === "undefined" ? undefined : globalThis.crypto;
  if (cryptoApi?.randomUUID) {
    return cryptoApi.randomUUID();
  }
  const bytes = new Uint8Array(16);
  if (cryptoApi?.getRandomValues) {
    cryptoApi.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `wg-${Date.now().toString(36)}-${hex}`;
}

/** 新建一页草稿：id 立即生成，字段留空等待用户填写。 */
export function createEntry(): WordEntry {
  return { id: createEntryId(), word: "", phonetic: "", meaning: "", example: "" };
}

/** 页面是否已完整到可以物化卡片（单词与释义都非空）。 */
export function isEntryComplete(entry: WordEntry): boolean {
  return entry.word.trim().length > 0 && entry.meaning.trim().length > 0;
}

/**
 * 解析 .qword 文本。
 *
 * 只校验"能不能安全编辑"的基本形状：JSON 对象、版本不高于当前、words 是对象数组、
 * 每页有合法 id、已知字段是字符串。其它越界（超长、id 重复）留给后端在保存时报错，
 * 避免前端静默改写用户数据。
 */
export function parseWordGroup(content: string): WordGroupParseResult {
  let raw: unknown;
  try {
    raw = JSON.parse(content);
  } catch {
    return { ok: false, error: "单词组文件不是合法的 JSON，已转为只读，不会覆盖原文件。" };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { ok: false, error: "单词组文件缺少顶层对象，已转为只读，不会覆盖原文件。" };
  }
  const record = raw as Record<string, unknown>;
  const rawVersion = record.formatVersion ?? WORD_GROUP_FORMAT_VERSION;
  if (typeof rawVersion !== "number" || !Number.isInteger(rawVersion) || rawVersion < 1) {
    return { ok: false, error: "单词组文件的 formatVersion 无效，已转为只读。" };
  }
  if (rawVersion > WORD_GROUP_FORMAT_VERSION) {
    return { ok: false, error: "单词组文件来自更新版本的应用，请升级后再编辑。" };
  }
  if (!Array.isArray(record.words)) {
    return { ok: false, error: "单词组文件缺少单词列表，已转为只读。" };
  }
  const words: WordEntry[] = [];
  for (let index = 0; index < record.words.length; index += 1) {
    const item: unknown = record.words[index];
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      return { ok: false, error: `第 ${index + 1} 页不是有效对象，已转为只读。` };
    }
    const entry = item as Record<string, unknown>;
    const id = entry.id;
    if (typeof id !== "string" || !ENTRY_ID_PATTERN.test(id)) {
      return { ok: false, error: `第 ${index + 1} 页缺少合法的 id，已转为只读。` };
    }
    const word = readText(entry, "word");
    const phonetic = readText(entry, "phonetic");
    const meaning = readText(entry, "meaning");
    const example = readText(entry, "example");
    if (word === null || phonetic === null || meaning === null || example === null) {
      return { ok: false, error: `第 ${index + 1} 页的字段类型不正确，已转为只读。` };
    }
    words.push({ id, word, phonetic, meaning, example });
  }
  return { ok: true, document: { formatVersion: rawVersion, words } };
}

/**
 * 序列化为磁盘格式。
 *
 * 稳定字段序 + 2 空格缩进 + 末尾换行，保证同样的内容永远得到同样的字节，
 * 保存服务才能用内容比较判断"真的变了"，避免无意义写盘。
 */
export function serializeWordGroup(document: WordGroupDocument): string {
  const payload = {
    formatVersion: WORD_GROUP_FORMAT_VERSION,
    words: document.words.map((entry) => ({
      id: entry.id,
      word: entry.word,
      phonetic: entry.phonetic,
      meaning: entry.meaning,
      example: entry.example,
    })),
  };
  return `${JSON.stringify(payload, null, 2)}\n`;
}

/** 音标定界符对：只接受成对的 /…/ 或 […]，其它只补不删。 */
const PHONETIC_DELIMITERS: ReadonlyArray<readonly [string, string]> = [["/", "/"], ["[", "]"]];

/** 统一 IPA 字符并补齐定界符：'→ˈ、ә→ə、:→ː，已有定界符保留。 */
export function normalizePhonetic(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "";
  }
  const normalized = trimmed
    .replace(/['’`ʻ]/g, "ˈ")
    .replace(/ә/g, "ə")
    .replace(/:/g, "ː");
  for (const [open, close] of PHONETIC_DELIMITERS) {
    if (!normalized.startsWith(open)) continue;
    // 已有开头定界符：只补缺失的结尾，绝不重复包裹。
    return normalized.endsWith(close) && normalized.length > 1 ? normalized : normalized + close;
  }
  // 只有结尾定界符（用户从别处复制了半个）时保持原样，避免再套一层。
  if (PHONETIC_DELIMITERS.some(([, close]) => normalized.endsWith(close))) {
    return normalized;
  }
  return `/${normalized}/`;
}
