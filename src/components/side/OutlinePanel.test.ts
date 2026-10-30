import { mount } from "@vue/test-utils";
import { afterEach, expect, test } from "vitest";
import type { OutlineItem } from "../../domain/outline";
import OutlinePanel from "./OutlinePanel.vue";

const items: OutlineItem[] = [
  { level: 1, text: "第一章", from: 0 },
  { level: 2, text: "第一节", from: 12 },
  { level: 3, text: "一个很长很长需要省略显示的标题", from: 40 },
];

const mounted: ReturnType<typeof mount>[] = [];
/** 每个测试释放组件，避免残留 DOM 与事件监听。 */
afterEach(() => { mounted.splice(0).forEach((wrapper) => wrapper.unmount()); });

/** 挂载大纲面板；默认用真实层级数据，避免测试只验证桩结构。 */
function panel(list: OutlineItem[] = items, activeFrom: number | null = null) {
  const wrapper = mount(OutlinePanel, { props: { items: list, activeFrom } });
  mounted.push(wrapper);
  return wrapper;
}

/** 点击标题只发送该标题的起始偏移，跳转动作由父层交给编辑器。 */
test("点击标题发送跳转偏移", async () => {
  const wrapper = panel();
  await wrapper.findAll("button")[1].trigger("click");
  expect(wrapper.emitted("jump")).toEqual([[items[1].from]]);
});

/** 当前小节用 aria-current 表达，同时有可见高亮类，读屏与视觉都不依赖颜色。 */
test("activeFrom 命中时高亮当前项", () => {
  const wrapper = panel(items, items[1].from);
  const buttons = wrapper.findAll("button");
  expect(buttons[0].attributes("aria-current")).toBeUndefined();
  expect(buttons[1].attributes("aria-current")).toBe("true");
  expect(buttons[1].classes()).toContain("bg-bg-active/60");
  expect(buttons[2].attributes("aria-current")).toBeUndefined();
});

/** 没有标题时给出空状态，不渲染任何可点击项。 */
test("空大纲显示没有标题", () => {
  const wrapper = panel([], null);
  expect(wrapper.text()).toBe("没有标题");
  expect(wrapper.findAll("button")).toHaveLength(0);
});

/** 层级用缩进表达且长标题单行省略，保证大纲在窄侧栏里仍可扫读。 */
test("按层级缩进并省略长标题", () => {
  const wrapper = panel();
  const buttons = wrapper.findAll("button");
  expect(buttons[0].attributes("style")).toContain("padding-left: 8px");
  expect(buttons[1].attributes("style")).toContain("padding-left: 20px");
  expect(buttons[2].attributes("style")).toContain("padding-left: 32px");
  expect(buttons[2].classes()).toContain("truncate");
  expect(buttons[2].attributes("title")).toBe(items[2].text);
});

/** 视口顶部没有标题时（-1 或 null）不选中任何项，避免高亮残留。 */
test("无当前标题时不选中任何项", () => {
  for (const activeFrom of [null, -1]) {
    const wrapper = panel(items, activeFrom);
    expect(wrapper.findAll("[aria-current]")).toHaveLength(0);
    wrapper.unmount();
    mounted.splice(mounted.indexOf(wrapper), 1);
  }
});
