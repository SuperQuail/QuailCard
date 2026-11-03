import { ref } from "vue";
import * as backend from "../../api/backend";
import type { PracticeSettings } from "../../domain/types";

/**
 * 打字背诵偏好域：每词重复遍数与每页单词数。
 *
 * 契约：与其他全局设置一致，以后端 settings.toml 为准；启动时由应用编排层
 * 用 BootstrapData 覆盖默认值，写回失败时保留旧值并抛出可读错误。
 */
export const practiceSettings = ref<PracticeSettings>({ repeatsPerWord: 5, wordsPerPage: 3 });

/** 启动时用后端下发的值覆盖本地默认值。 */
export function applyPracticeSettings(settings: PracticeSettings): void {
  practiceSettings.value = { ...settings };
}

/** 保存偏好：成功以后端回显为准，失败保留旧值并向上抛出。 */
export async function setPracticeSettings(next: PracticeSettings): Promise<void> {
  practiceSettings.value = await backend.setPracticeSettings(next);
}
