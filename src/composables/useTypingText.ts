import { onBeforeUnmount, ref, watch } from "vue";

const MAX_CODE_POINTS_PER_FRAME = 256;
const FALLBACK_FRAME_MS = 16;

/** 网络快照只更新目标，每帧有界推进一次；正常完成可继续追赶，取消由 animate=false 即时同步。 */
export function useTypingText(source: () => string, animate: () => boolean) {
  const displayed = ref(animate() ? "" : source());
  let target = source();
  let cursor = displayed.value.length;
  let frame: number | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  /** 每帧只扫描有限码点，不拆代理对，也不为长积压展开整个剩余字符串。 */
  function tick(): void {
    frame = undefined;
    timer = undefined;
    if (disposed) return;
    const budget = Math.min(MAX_CODE_POINTS_PER_FRAME, Math.ceil((target.length - cursor) / 16));
    let end = cursor;
    for (let count = 0; count < budget && end < target.length; count++) {
      const point = target.codePointAt(end)!;
      // 网络块可能恰好停在高代理项，等后续快照补齐，不能展示半个 emoji。
      if (point >= 0xd800 && point <= 0xdbff && end + 1 === target.length) break;
      end += point > 0xffff ? 2 : 1;
    }
    const advanced = end > cursor;
    cursor = end;
    displayed.value = target.slice(0, cursor);
    // 仅剩高代理项时等待输入，不空转帧；新快照会再次唤醒。
    if (advanced) schedule();
  }

  /** 单一待执行帧合并输入突发；无完整 rAF API 时使用可被假时钟稳定驱动的 16ms 定时器。 */
  function schedule(): void {
    if (disposed || frame !== undefined || timer !== undefined) return;
    if (cursor === target.length && displayed.value === target) return;
    if (typeof requestAnimationFrame === "function" && typeof cancelAnimationFrame === "function") {
      frame = requestAnimationFrame(tick);
    } else {
      timer = setTimeout(tick, FALLBACK_FRAME_MS);
    }
  }

  /** 取消和卸载同时释放两种调度资源，避免旧回调写入已失效的显示状态。 */
  function cancel(): void {
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (timer !== undefined) clearTimeout(timer);
    frame = undefined;
    timer = undefined;
  }

  /** 前缀替换只重置游标，等下一帧原子发布，避免帧间清空触发额外 Markdown 解析。 */
  watch([source, animate], ([text, active]) => {
    target = text;
    if (!active) {
      cancel();
      cursor = text.length;
      displayed.value = text;
    } else {
      if (!text.startsWith(displayed.value)) cursor = 0;
      schedule();
    }
  }, { immediate: true });

  /** 卸载后的调度即使已经进入浏览器回调队列也不能继续推进。 */
  onBeforeUnmount(() => { disposed = true; cancel(); });
  return displayed;
}
