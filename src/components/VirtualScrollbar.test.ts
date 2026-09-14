import { mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref } from "vue";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import VirtualScrollbar from "./VirtualScrollbar.vue";

/** jsdom 没有指针捕获；拖拽用例用空实现顶替，生产代码不为测试让路。 */
beforeAll(() => {
  HTMLElement.prototype.setPointerCapture = () => {};
  HTMLElement.prototype.releasePointerCapture = () => {};
});

const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount());
  vi.useRealTimers();
});

type Orientation = "vertical" | "horizontal";

/** 宿主复刻真实用法：浮层与滚动容器同级渲染，模板 ref 先于子组件拿到 DOM。 */
function bar(orientation: Orientation = "vertical") {
  const strip = ref<HTMLElement | null>(null);
  const Host = defineComponent({
    setup() {
      return () => h("div", { class: "viewport" }, [
        h("div", { ref: strip, class: "strip" }),
        h(VirtualScrollbar, { target: strip.value, orientation }),
      ]);
    },
  });
  const wrapper = mount(Host, { attachTo: document.body });
  mounted.push(wrapper as unknown as ReturnType<typeof mount>);
  return { wrapper, strip: wrapper.get(".strip").element as HTMLElement };
}

/**
 * 直接派发指针事件：@vue/test-utils 的 trigger 会往事件对象上回写 readonly 的 button/clientX，
 * 在 jsdom 里直接抛错，所以拖拽链路用真实事件走一遍。
 */
async function pointer(element: Element, type: string, init: MouseEventInit & { pointerId?: number }): Promise<void> {
  element.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, ...init }));
  await nextTick();
}

/** 用例只关心自己那几项几何，其余走默认值。 */
type Metrics = { client: number; content: number; scroll?: number; box?: { left: number; top: number; width: number; height: number } };

/** jsdom 不排版：手动伪造容器几何，再补一个 scroll 事件让组件重测（真实浏览器由滚动自身触发）。 */
async function layout(strip: HTMLElement, metrics: Metrics, orientation: Orientation = "vertical"): Promise<void> {
  const [view, content, scroll] = orientation === "vertical"
    ? ["clientHeight", "scrollHeight", "scrollTop"]
    : ["clientWidth", "scrollWidth", "scrollLeft"];
  const fields: Record<string, number> = { [view]: metrics.client, [content]: metrics.content };
  const target = strip as unknown as Record<string, number>;
  Object.entries(fields).forEach(([key, value]) => Object.defineProperty(strip, key, { value, configurable: true }));
  if (metrics.scroll !== undefined) target[scroll] = metrics.scroll;
  if (metrics.box) {
    Object.defineProperty(strip, "offsetLeft", { value: metrics.box.left, configurable: true });
    Object.defineProperty(strip, "offsetTop", { value: metrics.box.top, configurable: true });
    Object.defineProperty(strip, "offsetWidth", { value: metrics.box.width, configurable: true });
    Object.defineProperty(strip, "offsetHeight", { value: metrics.box.height, configurable: true });
  }
  strip.dispatchEvent(new Event("scroll"));
  await nextTick();
}

/** 浮层虚拟滚动条；不溢出时应当完全不存在。 */
function overlay(wrapper: ReturnType<typeof mount>) {
  return wrapper.find("[data-virtual-scrollbar]");
}

/** 实际渲染出的 thumb 元素，拖拽与样式断言都基于它。 */
function thumb(wrapper: ReturnType<typeof mount>): HTMLElement {
  return wrapper.get(".virtual-thumb").element as HTMLElement;
}

/** 不溢出就不该有浮层，免得它盖住窗格边缘的点击。 */
test("内容不溢出时不渲染滚动条", async () => {
  const { wrapper, strip } = bar();
  await layout(strip, { client: 300, content: 300 });
  expect(overlay(wrapper).exists()).toBe(false);
});

/** 浮层是同级兄弟：坐标直接照搬 target 的 offset*，窗格位置变了也不会错位。 */
test("浮层盒子对齐同级滚动容器的 offset 几何", async () => {
  const { wrapper, strip } = bar();
  await layout(strip, { client: 200, content: 800, box: { left: 12, top: 30, width: 200, height: 400 } });
  const style = overlay(wrapper).attributes("style") ?? "";
  expect(style).toContain("left: 12px");
  expect(style).toContain("top: 30px");
  expect(style).toContain("width: 200px");
  expect(style).toContain("height: 400px");
  expect(overlay(wrapper).attributes("data-orientation")).toBe("vertical");
});

/** 纵向：thumb 高度按视口占内容的比例，位移按剩余行程等比换算。 */
test("纵向按比例给出 thumb 高度与位移", async () => {
  const { wrapper, strip } = bar();
  await layout(strip, { client: 200, content: 800 });
  expect(overlay(wrapper).attributes("data-scroll-max")).toBe("600");
  expect(overlay(wrapper).attributes("data-thumb-length")).toBe("50");
  expect(overlay(wrapper).attributes("data-thumb-offset")).toBe("0");
  expect(thumb(wrapper).style.transform).toBe("translateY(0px)");
  expect(thumb(wrapper).style.height).toBe("50px");
  expect(thumb(wrapper).style.width).toBe("");

  await layout(strip, { client: 200, content: 800, scroll: 600 });
  expect(overlay(wrapper).attributes("data-thumb-offset")).toBe("150");
  expect(thumb(wrapper).style.transform).toBe("translateY(150px)");
});

/** 横向沿用同一套换算，只是换轴：thumb 宽度与 translateX。 */
test("横向按比例给出 thumb 宽度与位移", async () => {
  const { wrapper, strip } = bar("horizontal");
  await layout(strip, { client: 200, content: 800, scroll: 300 }, "horizontal");
  expect(overlay(wrapper).attributes("data-orientation")).toBe("horizontal");
  expect(thumb(wrapper).style.width).toBe("50px");
  expect(thumb(wrapper).style.transform).toBe("translateX(75px)"); // 300/600 × 150
  expect(thumb(wrapper).style.left).toBe("");
});

/** thumb 被最短长度兜底后拖拽比例随之修正：拖满剩余行程正好把滚动量用尽。 */
test("纵向拖拽 thumb 按修正比例滚动并夹住末端", async () => {
  const { wrapper, strip } = bar();
  await layout(strip, { client: 100, content: 2000 });
  expect(overlay(wrapper).attributes("data-thumb-length")).toBe("28");
  await pointer(thumb(wrapper), "pointerdown", { button: 0, clientY: 100, pointerId: 1 });
  expect(overlay(wrapper).classes()).toContain("is-dragging");
  await pointer(thumb(wrapper), "pointermove", { clientY: 172, pointerId: 1 });
  expect(strip.scrollTop).toBeCloseTo(1900); // 1900 / (100 - 28) 的放大倍数
  await pointer(thumb(wrapper), "pointerup", { pointerId: 1 });
  expect(overlay(wrapper).classes()).not.toContain("is-dragging");
});

/** 点贴边窄带先把 thumb 中心对到指针处，再顺势拖拽，抓点不会跳。 */
test("点击贴边窄带按位置跳转并继续拖拽", async () => {
  const { wrapper, strip } = bar();
  await layout(strip, { client: 200, content: 800 });
  const track = wrapper.get(".virtual-track").element as HTMLElement;
  track.getBoundingClientRect = () => ({ top: 100 }) as DOMRect;
  await pointer(track, "pointerdown", { button: 0, clientY: 225, pointerId: 2 });
  // 指针落在轨道 125px 处，居中后是 125 - 半长 25 = 100px，占 150px 行程的三分之二 → 600 的三分之二。
  expect(strip.scrollTop).toBeCloseTo(400);
  // 紧接着的拖动按 600/150 = 4 倍放大：再拖 15px 就再滚 60。
  await pointer(track, "pointermove", { clientY: 240, pointerId: 2 });
  expect(strip.scrollTop).toBeCloseTo(460);
});

/** 悬停立刻显示，滚过之后延时淡出；指针移到贴边窄带上也不能把 thumb 弄没。 */
test("悬停显示、滚动后延时隐藏、窄带悬停保持显示", async () => {
  vi.useFakeTimers();
  const { wrapper, strip } = bar();
  await layout(strip, { client: 200, content: 800 });
  expect(overlay(wrapper).classes()).not.toContain("is-visible");

  strip.dispatchEvent(new Event("pointerenter"));
  await nextTick();
  expect(overlay(wrapper).classes()).toContain("is-visible");

  strip.dispatchEvent(new Event("pointerleave"));
  await nextTick();
  expect(overlay(wrapper).classes()).not.toContain("is-visible");

  wrapper.get(".virtual-track").element.dispatchEvent(new Event("pointerenter"));
  await nextTick();
  expect(overlay(wrapper).classes()).toContain("is-visible");

  strip.dispatchEvent(new Event("scroll"));
  await nextTick();
  expect(overlay(wrapper).classes()).toContain("is-visible");
  vi.advanceTimersByTime(1000);
  await nextTick();
  expect(overlay(wrapper).classes()).toContain("is-visible"); // 窄带还悬停着，不该淡出

  wrapper.get(".virtual-track").element.dispatchEvent(new Event("pointerleave"));
  await nextTick();
  expect(overlay(wrapper).classes()).not.toContain("is-visible");
});

/** 容器被替换（标签条或窗格重建）时旧监听必须撤掉，否则几何会被上一个容器写脏。 */
test("target 换成新容器后按新容器测量", async () => {
  const first = ref<HTMLElement | null>(null);
  const second = ref<HTMLElement | null>(null);
  const useSecond = ref(false);
  const Host = defineComponent({
    setup() {
      return () => h("div", { class: "viewport" }, [
        h("div", { ref: first, class: "strip a" }),
        h("div", { ref: second, class: "strip b" }),
        h(VirtualScrollbar, { target: useSecond.value ? second.value : first.value }),
      ]);
    },
  });
  const wrapper = mount(Host, { attachTo: document.body });
  mounted.push(wrapper as unknown as ReturnType<typeof mount>);
  await layout(wrapper.get(".strip.a").element as HTMLElement, { client: 100, content: 400 });
  expect(overlay(wrapper).attributes("data-thumb-length")).toBe("28");
  useSecond.value = true;
  await nextTick();
  await layout(wrapper.get(".strip.b").element as HTMLElement, { client: 400, content: 400 });
  expect(overlay(wrapper).exists()).toBe(false);
});
