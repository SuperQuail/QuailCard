import { synthesizeSpeech } from "../api/backend";

/** 正在播放的音频：新发音打断旧的，避免叠音。 */
let playing: HTMLAudioElement | null = null;

/** 朗读英文单词：优先后端合成，空结果或失败回退浏览器内置语音。 */
export async function pronounce(word: string): Promise<void> {
  const trimmed = word.trim();
  if (!trimmed) {
    return;
  }
  playing?.pause();
  playing = null;
  try {
    const dataUrl = await synthesizeSpeech(trimmed);
    if (dataUrl) {
      playing = new Audio(dataUrl);
      await playing.play();
      return;
    }
  } catch {
    // 后端合成失败时回退浏览器语音，朗读体验不应因为一次失败消失。
  }
  const utterance = new SpeechSynthesisUtterance(trimmed);
  utterance.lang = "en-US";
  window.speechSynthesis?.speak(utterance);
}
