import { synthesizeSpeech } from "../api/backend";

/**
 * 打字背诵的发音缓存。
 *
 * 契约：只做「已准备好才播放」——进入练习前并发预取前 N 个单词，练习中每完成一个词
 * 后台补一个；没准备就静默跳过，绝不为了发音而拖慢打字。
 * 后端语音服务（Edge TTS）不可用时，改用系统语音朗读同一条词，保证练习始终有声音。
 */

/** 单词 → 音频 Data URL；键已 trim。 */
const cache = new Map<string, string>();
/** 后端合成失败、改用系统语音朗读的单词：避免每次完成都重复请求。 */
const fallback = new Set<string>();
/** 正在播放的音频：新发音打断旧的，避免叠音。 */
let playing: HTMLAudioElement | null = null;
/** 后台补缓存的串行队列：一次只合成一个，避免和界面上的预取抢带宽。 */
let chain: Promise<void> = Promise.resolve();

/** 单词是否已有可播放的发音（真实音频或系统语音兜底）。 */
export function hasCachedAudio(word: string): boolean {
  const target = word.trim();
  return cache.has(target) || fallback.has(target);
}

/** 已准备数量：仅测试与调试使用。 */
export function cachedAudioCount(): number {
  return cache.size + fallback.size;
}

/** 请求一个词的发音并写入缓存；失败时登记为系统语音兜底，避免重复请求。 */
async function loadWord(word: string): Promise<void> {
  if (hasCachedAudio(word)) {
    return;
  }
  try {
    const dataUrl = await synthesizeSpeech(word);
    if (dataUrl) {
      cache.set(word, dataUrl);
      return;
    }
  } catch {
    // 后端语音不可用时走系统语音，下面统一登记。
  }
  fallback.add(word);
}

/**
 * 预取一批单词的发音。
 * 契约：并发请求；单个失败改用系统语音兜底，不影响其他词与练习流程；
 * 进度回调按完成个数递增，供开始按钮显示准备进度。
 */
export async function preloadAudio(words: string[], onProgress?: (done: number, total: number) => void): Promise<void> {
  const targets = [...new Set(words.map((word) => word.trim()).filter(Boolean))];
  let done = 0;
  onProgress?.(done, targets.length);
  await Promise.all(targets.map(async (word) => {
    await loadWord(word);
    done += 1;
    onProgress?.(done, targets.length);
  }));
}

/** 后台补一个单词的发音：串行排队，已准备的词直接跳过。 */
export function cacheAudioInBackground(word: string): void {
  const target = word.trim();
  if (!target || hasCachedAudio(target)) {
    return;
  }
  chain = chain.then(() => loadWord(target));
}

/** 用系统语音朗读：后端合成失败时的兜底，不做网络等待。 */
function speakWithSystemVoice(word: string): void {
  try {
    const synth = typeof window !== "undefined" ? window.speechSynthesis : undefined;
    if (!synth || typeof SpeechSynthesisUtterance === "undefined") {
      return;
    }
    synth.cancel();
    const utterance = new SpeechSynthesisUtterance(word);
    utterance.lang = "en-US";
    utterance.rate = 0.95;
    synth.speak(utterance);
  } catch {
    // 系统语音不可用就静默，练习不能因为朗读失败而中断。
  }
}

/** 播放已准备发音；既没有缓存也不在兜底集合里时直接跳过（打得快就不播）。 */
export function playCachedAudio(word: string): void {
  const target = word.trim();
  const dataUrl = cache.get(target);
  if (!dataUrl) {
    if (fallback.has(target)) {
      speakWithSystemVoice(target);
    }
    return;
  }
  try {
    playing?.pause();
    const audio = new Audio(dataUrl);
    playing = audio;
    const played = audio.play();
    // 浏览器可能返回 Promise（自动播放策略会拒绝），也要容忍不返回 Promise 的环境。
    if (played && typeof (played as Promise<void>).catch === "function") {
      (played as Promise<void>).catch(() => {
        playing = null;
      });
    }
  } catch {
    playing = null;
  }
}

/** 清空缓存与播放器：测试与显式重置使用。 */
export function resetAudioCache(): void {
  cache.clear();
  fallback.clear();
  chain = Promise.resolve();
  playing?.pause();
  playing = null;
  try {
    window.speechSynthesis?.cancel();
  } catch {
    // 测试环境没有语音合成时忽略。
  }
}
