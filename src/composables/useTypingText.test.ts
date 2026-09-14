import { afterEach, expect, test, vi } from "vitest";
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref } from "vue";
import { useTypingText } from "./useTypingText";

enableAutoUnmount(afterEach);
/** 清理假时钟与全局帧 API，避免跨例泄漏。 */
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

/** 显式驱动帧边界，回调中新排的帧只能在下次驱动时执行。 */
function frames() {
  const pending = new Map<number, FrameRequestCallback>();
  let id = 0;
  vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => { pending.set(++id, callback); return id; }));
  vi.stubGlobal("cancelAnimationFrame", vi.fn((key: number) => pending.delete(key)));
  return { pending, async step() {
    const callbacks = [...pending.values()]; pending.clear();
    for (const callback of callbacks) callback(16);
    await nextTick();
  } };
}

/** 真实组件生命周期承载组合函数，输出引用用于检查卸载后的写入。 */
function typing(initial: string, animated = true) {
  const text = ref(initial);
  const active = ref(animated);
  let displayed = ref("");
  const wrapper = mount(defineComponent({
    setup() { displayed = useTypingText(() => text.value, () => active.value); return () => h("div", displayed.value); },
  }));
  return { text, active, displayed, wrapper };
}

// 百万级积压不能展开全量码点或用循环在同一帧追完。
test("大积压每帧最多扫描256码点，多次输入只保留一个待执行帧", async () => {
  const clock = frames();
  const state = typing("😀".repeat(100_000));
  const scan = vi.spyOn(String.prototype, "codePointAt");
  for (let index = 0; index < 20; index++) { state.text.value += "中"; await nextTick(); }
  expect(clock.pending.size).toBe(1);
  expect(state.displayed.value).toBe("");
  await clock.step();
  expect(scan).toHaveBeenCalledTimes(256);
  expect(state.displayed.value).toBe("😀".repeat(256));
  await nextTick();
  expect(scan).toHaveBeenCalledTimes(256);
  await clock.step();
  expect(scan).toHaveBeenCalledTimes(512);
  expect(state.displayed.value).toBe("😀".repeat(512));
  expect(clock.pending.size).toBe(1);
});

// 停止收到网络块后仍应有限帧追完，而非等待新的输入唤醒。
test("最终快照保持动画时继续追赶并在队列耗尽后停止", async () => {
  const clock = frames();
  const state = typing("中😀".repeat(1000));
  let count = 0;
  while (clock.pending.size && count++ < 200) await clock.step();
  expect(state.displayed.value).toBe(state.text.value);
  expect(count).toBeLessThan(200);
  expect(clock.pending.size).toBe(0);
});

// 保持码点完整包含网络快照恰好在 UTF-16 代理对中间断开的情况。
test("短文本不拆emoji，末尾半个代理对等待下一块而不空转", async () => {
  const clock = frames();
  const state = typing("你\ud83d");
  await clock.step(); expect(state.displayed.value).toBe("你");
  await clock.step(); expect(clock.pending.size).toBe(0);
  state.text.value = "你😀好"; await nextTick();
  await clock.step(); expect(state.displayed.value).toBe("你😀");
  await clock.step(); expect(state.displayed.value).toBe("你😀好");
});

// 替换包括空串；同帧多次替换只应发布最后一个目标的前缀。
test("替换前缀在帧边界重置，清空已追完文本也能发布", async () => {
  const clock = frames();
  const state = typing("旧旧旧");
  await clock.step();
  state.text.value = "中间"; await nextTick();
  state.text.value = "新😀"; await nextTick();
  expect(state.displayed.value).toBe("旧");
  await clock.step(); expect(state.displayed.value).toBe("新");
  await clock.step(); expect(state.displayed.value).toBe("新😀");
  state.text.value = ""; await nextTick();
  await clock.step(); expect(state.displayed.value).toBe("");
  expect(clock.pending.size).toBe(0);
});

// 历史与取消不能依赖浏览器是否继续提供帧（如页面被后台挂起）。
test("历史立即显示，取消立即补齐并撤销帧，卸载拒绝迟到回调", async () => {
  const clock = frames();
  const state = typing("历史😀", false);
  expect(state.displayed.value).toBe("历史😀"); expect(clock.pending.size).toBe(0);
  state.active.value = true; state.text.value += "新".repeat(1000); await nextTick();
  state.active.value = false; await nextTick();
  expect(state.displayed.value).toBe(state.text.value); expect(clock.pending.size).toBe(0);
  state.active.value = true; state.text.value += "尾"; await nextTick();
  const late = [...clock.pending.values()][0];
  const before = state.displayed.value;
  state.wrapper.unmount();
  expect(clock.pending.size).toBe(0);
  late(16); await nextTick();
  expect(state.displayed.value).toBe(before);
});

// 没有 rAF 的测试环境仍有稳定的16ms批次，并可以彻底释放定时器。
test("无rAF退化为单一定时器，取消和卸载清理积压", async () => {
  vi.stubGlobal("requestAnimationFrame", undefined); vi.stubGlobal("cancelAnimationFrame", undefined);
  vi.useFakeTimers();
  const state = typing("中".repeat(10_000));
  expect(vi.getTimerCount()).toBe(1);
  await vi.advanceTimersByTimeAsync(15); expect(state.displayed.value).toBe("");
  await vi.advanceTimersByTimeAsync(1); expect(state.displayed.value.length).toBe(256);
  state.active.value = false; await nextTick();
  expect(state.displayed.value).toBe(state.text.value); expect(vi.getTimerCount()).toBe(0);
  state.active.value = true; state.text.value += "新"; await nextTick();
  state.wrapper.unmount(); expect(vi.getTimerCount()).toBe(0);
});
