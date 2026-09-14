/** 从文本里识别音标并给出可朗读的单词；纯函数，便于测试与复用。 */

export interface PhoneticSegment {
  text: string;
  /** 命中音标时保留原文，便于原样展示。 */
  phonetic?: string;
  /** 前面的英文单词；没有就不贴朗读按钮。 */
  word?: string;
}

/** IPA 专用字符：至少要命中一个才认为是音标，避免 [1]、【注】 被当成发音。 */
const IPA_CHARS = /[ˈˌːəɪʊʌɔæɑɒɜɐɞɘɵøœɶɛɤɯɨʉɚɝɹɾɻʀχʁħʕʔʧʤʦʣθðʃʒŋɲçʝɟ]/;

/** 音标分隔符：斜杠或方括号，内部不含分隔符与换行且长度有限。 */
const PHONETIC = /[/[]([^/\][\n]{1,40})[/\]]/g;

/** 是否像音标：分隔符 + IPA 字符，且不含中文（中文方括号里的注解不算）。 */
export function isPhonetic(value: string): boolean {
  const trimmed = value.trim();
  const match = /^[/[]([^/\][\n]{1,40})[/\]]$/.exec(trimmed);
  if (!match) return false;
  return IPA_CHARS.test(match[1]) && !/[\u4e00-\u9fff]/.test(match[1]);
}

/** 音标前的单词：同一行内向前找最近的英文词，"speak 读 /spiːk/" 这类中文间隔也算。 */
function wordBefore(text: string, index: number): string | undefined {
  const lineStart = text.lastIndexOf("\n", index - 1) + 1;
  const head = text.slice(lineStart, index);
  return /([A-Za-z][A-Za-z'’-]*)[^\nA-Za-z]*$/.exec(head)?.[1];
}

/** 一段正文里的英文单词；先剔除音标，避免把 IPA 里的字母当成单词。 */
export function latinWords(text: string): string[] {
  const plain = splitPhonetics(text)
    .filter(piece => !piece.phonetic)
    .map(piece => piece.text)
    .join(" ");
  return plain.match(/[A-Za-z][A-Za-z'’-]{2,}/g) ?? [];
}

/** 把文本切成「普通文字」与「音标」片段；音标片段的 word 可用于朗读。 */
export function splitPhonetics(text: string): PhoneticSegment[] {
  const result: PhoneticSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(PHONETIC)) {
    const token = match[0];
    if (!isPhonetic(token)) continue;
    const index = match.index ?? 0;
    result.push({ text: text.slice(cursor, index) });
    result.push({ text: token, phonetic: token, word: wordBefore(text, index) });
    cursor = index + token.length;
  }
  result.push({ text: text.slice(cursor) });
  return result.filter(segment => segment.text.length > 0 || segment.word);
}
