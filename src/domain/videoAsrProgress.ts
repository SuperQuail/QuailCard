import type { AsrProgress } from "./video";

/** 只有有效 Whisper 回调才能计算展示量；整数进度与元数据时长意味着近似值。 */
export function asrMeasurements(progress: AsrProgress) {
  const percent = typeof progress.percent === "number" && Number.isInteger(progress.percent)
    && progress.percent >= 0 && progress.percent <= 100 ? progress.percent : null;
  const total = typeof progress.totalAudioSeconds === "number" && Number.isFinite(progress.totalAudioSeconds)
    && progress.totalAudioSeconds > 0 ? progress.totalAudioSeconds : null;
  const elapsed = Number.isFinite(progress.elapsedSeconds) && progress.elapsedSeconds >= 0 ? progress.elapsedSeconds : 0;
  const processed = percent !== null && total !== null ? total * percent / 100 : null;
  const speed = processed !== null && processed > 0 && elapsed >= 1 ? processed / elapsed : null;
  return { percent, total, elapsed, processed, speed };
}
