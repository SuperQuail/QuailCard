import { ref } from "vue";

/** 全局轻提示状态：同一条消息在超时后自动消失。 */
const toastMessage = ref("");

/** 显示短暂反馈（2.2 秒后自动消失，重复消息重置计时）。 */
export function useToast() {
  function showToast(message: string): void {
    toastMessage.value = message;
    window.setTimeout(() => {
      if (toastMessage.value === message) {
        toastMessage.value = "";
      }
    }, 2200);
  }
  return { toastMessage, showToast };
}
