import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import { h, nextTick } from "vue";
import AgentVirtualList from "./AgentVirtualList.vue";

/** 行内容只回显下标：窗口与 spacer 的断言不依赖任何业务字段。 */
function cell(params: { index: number }) { return [h("span", { class: "cell" }, String(params.index))]; }

/** 列表几何契约；用例只覆盖需要的字段，其余走默认值。 */
type ListProps = { count: number; itemHeight: number; viewportHeight: number; overscan?: number; minWindow?: number; ariaLabel?: string };

/** 挂载固定几何的列表：默认 100 行 × 20px、视口 100px，即默认开启窗口化。 */
function mountList(props: Partial<ListProps> = {}) {
  return mount(AgentVirtualList, {
    props: { count: 100, itemHeight: 20, viewportHeight: 100, ...props },
    slots: { default: cell },
  });
}

/** jsdom 的 rAF 走定时器：等一帧并冲刷 Vue 更新后再断言窗口位移。 */
async function afterFrame(): Promise<void> {
  await new Promise<void>(resolve => {
    requestAnimationFrame(() => resolve());
    setTimeout(resolve, 40);
  });
  await nextTick();
}

/** 某个 spacer 的实际像素高度；未渲染时返回空串，便于断言“不该有 spacer”。 */
function spacerHeight(wrapper: ReturnType<typeof mountList>, side: "top" | "bottom"): string {
  const spacer = wrapper.find(`[data-virtual-spacer="${side}"]`);
  return spacer.exists() ? (spacer.element as HTMLElement).style.height : "";
}

// 行数不超 minWindow：小列表全量渲染，不产生 spacer。
test("未超窗口时全量渲染且窗口覆盖全部行", () => {
  const wrapper = mountList({ count: 3 });
  expect(wrapper.attributes("data-virtual-start")).toBe("0");
  expect(wrapper.attributes("data-virtual-end")).toBe("3");
  expect(wrapper.attributes("data-virtual-total")).toBe("3");
  expect(wrapper.findAll('[role="listitem"]')).toHaveLength(3);
  expect(wrapper.findAll(".cell").map(node => node.text())).toEqual(["0", "1", "2"]);
  expect(spacerHeight(wrapper, "top")).toBe("");
  expect(spacerHeight(wrapper, "bottom")).toBe("");
});

// 行数超过 minWindow 但总高不超视口：根本不需要滚动，同样全量渲染。
test("总高不超过视口时也全量渲染", () => {
  const wrapper = mountList({ count: 8, itemHeight: 10, viewportHeight: 100 });
  expect(wrapper.attributes("data-virtual-start")).toBe("0");
  expect(wrapper.attributes("data-virtual-end")).toBe("8");
  expect(wrapper.findAll('[role="listitem"]')).toHaveLength(8);
  expect(wrapper.find('[data-virtual-spacer]').exists()).toBe(false);
});

// 超窗口：只渲染窗口行，上下 spacer 分别等于窗口之前与之后被裁掉的行高之和。
test("超窗口时只渲染窗口行且 spacer 高度等于被裁行高", () => {
  const wrapper = mountList();
  expect(wrapper.attributes("data-virtual-start")).toBe("0");
  expect(wrapper.attributes("data-virtual-end")).toBe("9"); // ceil(100 / 20) + overscan 4
  expect(wrapper.findAll('[role="listitem"]')).toHaveLength(9);
  expect(wrapper.findAll(".cell").map(node => node.text())).toEqual(["0", "1", "2", "3", "4", "5", "6", "7", "8"]);
  expect(spacerHeight(wrapper, "top")).toBe("0px");
  expect(spacerHeight(wrapper, "bottom")).toBe(`${(100 - 9) * 20}px`);
  expect((wrapper.get('[role="listitem"]').element as HTMLElement).style.height).toBe("20px");
});

// 滚动后窗口前移：start 退 overscan 行、end 进 overscan 行，spacer 跟着换算。
test("滚动后窗口前移且行数不变", async () => {
  const wrapper = mountList();
  const list = wrapper.get('[role="list"]');
  (list.element as HTMLElement).scrollTop = 400;
  await list.trigger("scroll");
  await afterFrame();
  expect(wrapper.attributes("data-virtual-start")).toBe("16"); // floor(400 / 20) - 4
  expect(wrapper.attributes("data-virtual-end")).toBe("29"); // ceil((400 + 100) / 20) + 4
  expect(wrapper.findAll('[role="listitem"]')).toHaveLength(13);
  expect(spacerHeight(wrapper, "top")).toBe("320px");
  expect(spacerHeight(wrapper, "bottom")).toBe(`${(100 - 29) * 20}px`);
  expect(wrapper.findAll(".cell").map(node => node.text())[0]).toBe("16");
});

// 列表变短：滚动位置先被夹到新的上限，窗口随之回收到可达范围，不会停在已消失的行上。
test("count 变小时滚动位置与窗口一起夹紧", async () => {
  const wrapper = mountList();
  const list = wrapper.get('[role="list"]');
  (list.element as HTMLElement).scrollTop = 1000;
  await list.trigger("scroll");
  await afterFrame();
  expect(wrapper.attributes("data-virtual-start")).toBe("46"); // floor(1000 / 20) - 4
  await wrapper.setProps({ count: 20 });
  expect((list.element as HTMLElement).scrollTop).toBe(300); // 20 * 20 - 100
  expect(wrapper.attributes("data-virtual-start")).toBe("11");
  expect(wrapper.attributes("data-virtual-end")).toBe("20");
  expect(wrapper.attributes("data-virtual-total")).toBe("20");
  expect(wrapper.findAll('[role="listitem"]')).toHaveLength(9);
  expect(spacerHeight(wrapper, "top")).toBe("220px");
  expect(spacerHeight(wrapper, "bottom")).toBe("0px");
});

// 容器被 flex 压缩时 prop 会失真：必须按实测高度收紧窗口，否则列表底部会出现滚不到的空白。
test("容器实测高度优先于视口 prop", async () => {
  let measure: ((height: number) => void) | undefined;
  class Observer {
    constructor(private readonly callback: (entries: { contentRect: { height: number } }[]) => void) {}
    /** 记录回调，测试自己决定何时上报真实高度。 */
    observe(): void { measure = (height: number) => this.callback([{ contentRect: { height } }]); }
    /** 卸载后不能再触发测量。 */
    disconnect(): void { measure = undefined; }
  }
  const scope = globalThis as { ResizeObserver?: unknown };
  scope.ResizeObserver = Observer;
  try {
    const wrapper = mountList({ count: 100, itemHeight: 20, viewportHeight: 100 });
    const wide = Number(wrapper.attributes("data-virtual-end"));
    measure?.(20);
    await nextTick();
    const narrow = Number(wrapper.attributes("data-virtual-end"));
    expect(narrow).toBeLessThan(wide);
    expect(narrow).toBe(5);
    expect(spacerHeight(wrapper, "bottom")).toBe("1900px");
  } finally {
    delete scope.ResizeObserver;
  }
});
