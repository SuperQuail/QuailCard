import { ref } from "vue";
import * as backend from "../../api/backend";
import type { FontSizeId, ThemeId } from "../../domain/types";

/**
 * UI 域 store：主题、字号与全局加载/错误反馈。
 * 只承载界面偏好与全局反馈状态，不涉及任何业务数据。
 */
export const theme = ref<ThemeId>(localStorage.getItem("quailcard.theme") === "dark" ? "dark" : "light");
export const fontSize = ref<FontSizeId>("comfortable");
export const loading = ref(false);
export const initialized = ref(false);
export const errorMessage = ref("");

/** 切换主题并持久化到本地存储。 */
export function toggleTheme(): void {
  theme.value = theme.value === "light" ? "dark" : "light";
  localStorage.setItem("quailcard.theme", theme.value);
  applyTheme();
}

/** 应用主题类名到根节点。 */
export function applyTheme(): void {
  document.documentElement.classList.toggle("dark", theme.value === "dark");
}

/** 设置字号档位：以后端持久化结果为准。 */
export async function setFontSize(next: FontSizeId): Promise<void> {
  fontSize.value = (await backend.setFontSize(next)) as FontSizeId;
  applyFontSize();
}

/** 应用字号 CSS 变量到正文容器。 */
export function applyFontSize(): void {
  const sizes: Record<FontSizeId, string> = {
    compact: "14px",
    standard: "16px",
    comfortable: "18px",
    large: "20px",
  };
  document.documentElement.style.setProperty("--editor-font-size", sizes[fontSize.value]);
}
