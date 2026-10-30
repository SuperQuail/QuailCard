import { ref } from "vue";

/** 通用确认请求：展示层只渲染文案，是否危险由调用方声明。 */
export interface ConfirmRequest {
  title: string;
  body: string;
  confirmLabel: string;
  /** 确认按钮是否使用危险色；删除类操作传 true，其余可省略（默认危险）。 */
  danger?: boolean;
}

/**
 * 壳层确认框状态：把「等待用户选择」的 Promise 与展示请求集中起来。
 *
 * 为什么独立成文件：App.vue 已接近行数上限，壳层只做接线；
 * 业务决策仍在用例层（useNoteActions / useNoteSidebars），本文件不含任何规则。
 */
export function useConfirm() {
  /** 当前待确认请求；null 表示确认框关闭。 */
  const request = ref<ConfirmRequest | null>(null);

  /** 挂起的 Promise 完成函数；同一时刻只允许一个确认框。 */
  let settleChoice: ((confirmed: boolean) => void) | null = null;

  /** 弹出确认框并等待用户选择；重复调用先取消上一次，避免 Promise 悬挂。 */
  function ask(next: ConfirmRequest): Promise<boolean> {
    settle(false);
    request.value = next;
    return new Promise((resolve) => { settleChoice = resolve; });
  }

  /** 关闭确认框并回传选择；先清状态再回传，连续点击也只生效一次。 */
  function settle(confirmed: boolean): void {
    const resolve = settleChoice;
    settleChoice = null;
    request.value = null;
    resolve?.(confirmed);
  }

  return { request, ask, settle };
}
