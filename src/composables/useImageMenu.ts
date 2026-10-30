import { ref } from "vue";
import type { ImageMenuRequest } from "../domain/imageMenu";
import { copyAttachmentImage, revealAttachment, saveAttachmentAs } from "../services/attachmentActions";
import { resolveError } from "../utils/errorMessage";

/** 图片菜单用例的依赖：只注入提示回调，菜单状态由本 composable 独占持有。 */
export interface ImageMenuOptions {
  showToast: (message: string) => void;
}

/**
 * 图片右键菜单用例：持有菜单上下文与忙碌标记，把三项动作转成 services 调用。
 *
 * 契约：
 * - 组件不得直接调用后端，只能经本 composable → src/services/attachmentActions.ts；
 * - 动作一开始就 close()，浮层立刻消失，用户不会看到菜单停在半空；
 * - busy 用 try/finally 复位，失败也必须解除禁用，否则菜单会永久卡死。
 *
 * 返回的 context 供壳层决定是否渲染菜单，busy 透传给展示组件。
 */
export function useImageMenu(options: ImageMenuOptions) {
  /** 当前菜单请求；null 表示菜单未打开。 */
  const context = ref<ImageMenuRequest | null>(null);
  /** 动作进行中：菜单项禁用，避免重复触发系统对话框或重复写剪贴板。 */
  const busy = ref(false);

  /** 打开菜单：请求由编辑器上报，含引用与已钳制进视口的坐标。 */
  function open(request: ImageMenuRequest): void {
    context.value = request;
  }

  /** 关闭菜单：用户点外部、按 Esc，或动作开始都走这里。 */
  function close(): void {
    context.value = null;
  }

  /**
   * 统一动作外壳：先取请求快照，再关闭菜单，最后在 finally 复位 busy。
   *
   * 取快照的原因：close() 会清空 context，而动作仍需要 notePath / source。
   * 没有请求时直接返回，防止重复触发（连点、重复快捷键）发出多余的后端调用。
   */
  async function run(action: (request: ImageMenuRequest) => Promise<void>): Promise<void> {
    const request = context.value;
    if (!request) {
      return;
    }
    close();
    busy.value = true;
    try {
      await action(request);
    } finally {
      busy.value = false;
    }
  }

  /** 复制图片：成功给一条轻提示，因为剪贴板没有可见结果可供用户自行确认。 */
  async function copyImage(): Promise<void> {
    await run(async (request) => {
      try {
        await copyAttachmentImage(request.notePath, request.source);
        options.showToast("已复制图片");
      } catch (error) {
        // 只提示 code + 安全 message，原始错误对象不进界面。
        options.showToast(resolveError(error));
      }
    });
  }

  /**
   * 另存为：只有拿到保存路径才提示，用户需要知道文件落在哪。
   *
   * 返回 null 表示用户在系统对话框里取消：这不算失败，静默处理，
   * 否则每次取消都弹一条「已取消」反而打扰用户。
   */
  async function saveImage(): Promise<void> {
    await run(async (request) => {
      try {
        const savedPath = await saveAttachmentAs(request.notePath, request.source);
        if (savedPath) {
          options.showToast(`已保存到 ${savedPath}`);
        }
      } catch (error) {
        options.showToast(resolveError(error));
      }
    });
  }

  /**
   * 在文件管理器中定位：成功不提示，系统窗口本身就是反馈，再弹提示是噪音。
   * 失败必须提示，否则用户点了没反应会以为菜单坏了。
   */
  async function revealImage(): Promise<void> {
    await run(async (request) => {
      try {
        await revealAttachment(request.notePath, request.source);
      } catch (error) {
        options.showToast(resolveError(error));
      }
    });
  }

  return { context, busy, open, close, copyImage, saveImage, revealImage };
}
