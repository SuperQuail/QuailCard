import { mount } from "@vue/test-utils";
import { h, nextTick } from "vue";
import { afterEach, expect, test } from "vitest";
import NoteTabs from "./NoteTabs.vue";
import NoteToolbar from "./NoteToolbar.vue";

const mounted: ReturnType<typeof mount>[] = [];
/** 每次卸载，避免焦点和异步标签定位污染后续用例。 */
afterEach(() => { mounted.splice(0).forEach((wrapper) => wrapper.unmount()); });
/** 使用真实标签组件和可读路径验证显示及事件，不注入业务状态。 */
function tabs() {
  const wrapper = mount(NoteTabs, { props: { tabs: [
    { path: "课程/绪论.md", title: "绪论" }, { path: "课程/线性表.md", title: "线性表" }, { path: "其他/绪论.md", title: "绪论" },
  ], activePath: "课程/绪论.md" }, attachTo: document.body });
  mounted.push(wrapper);
  return wrapper;
}

/** 标题缩略时仍通过路径区分同名笔记，激活标签才进入键盘焦点顺序。 */
test("展示已打开笔记、同名目录提示和当前标签", () => {
  const wrapper = tabs();
  expect(wrapper.get('[role="tablist"]').attributes("aria-label")).toBe("已打开的笔记");
  const buttons = wrapper.findAll('[role="tab"]');
  expect(buttons).toHaveLength(3);
  expect(buttons[0].attributes("aria-selected")).toBe("true");
  expect(buttons[0].attributes("tabindex")).toBe("0");
  expect(buttons[1].attributes("tabindex")).toBe("-1");
  expect(buttons[0].text()).toContain("课程");
  expect(buttons[2].text()).toContain("其他");
  expect(buttons[1].attributes("title")).toBe("课程/线性表.md");
  expect(wrapper.find("button button").exists()).toBe(false);
});

/** 当前文档被删除后，剩余标签仍可从键盘进入，不能全部变成负 tabindex。 */
test("没有当前笔记时仍保留标签焦点入口", async () => {
  const wrapper = tabs();
  await wrapper.setProps({ activePath: "" });
  const buttons = wrapper.findAll('[role="tab"]');
  expect(buttons[0].attributes("tabindex")).toBe("0");
  expect(buttons[0].attributes("aria-selected")).toBe("false");
  await buttons[0].trigger("click");
  expect(wrapper.emitted("select")).toEqual([["课程/绪论.md"]]);
});

/** 关闭独立于选择，右键不误关，中键沿用桌面编辑器关闭标签习惯。 */
test("点击切换、叉号关闭及中键关闭互不串事件", async () => {
  const wrapper = tabs();
  await wrapper.findAll('[role="tab"]')[1].trigger("click");
  expect(wrapper.emitted("select")).toEqual([["课程/线性表.md"]]);
  await wrapper.get('[aria-label="关闭标签：其他/绪论.md"]').trigger("click");
  expect(wrapper.emitted("close")).toEqual([["其他/绪论.md"]]);
  expect(wrapper.emitted("select")).toHaveLength(1);
  await wrapper.findAll(".note-tab")[0].trigger("auxclick", { button: 2 });
  expect(wrapper.emitted("close")).toHaveLength(1);
  await wrapper.findAll(".note-tab")[0].trigger("auxclick", { button: 1 });
  expect(wrapper.emitted("close")?.[1]).toEqual(["课程/绪论.md"]);
});

/** 键盘切换不触碰正文；Delete 的语义仅是关闭标签。 */
test("方向键与首尾键切换，Delete请求关闭", async () => {
  const wrapper = tabs();
  const buttons = wrapper.findAll('[role="tab"]');
  await buttons[0].trigger("keydown", { key: "ArrowLeft" });
  expect(wrapper.emitted("select")?.[0]).toEqual(["其他/绪论.md"]);
  expect(document.activeElement).toBe(buttons[2].element);
  await buttons[2].trigger("keydown", { key: "Home" });
  await buttons[0].trigger("keydown", { key: "End" });
  expect(wrapper.emitted("select")?.slice(1)).toEqual([["课程/绪论.md"], ["其他/绪论.md"]]);
  await buttons[2].trigger("keydown", { key: "Delete" });
  expect(wrapper.emitted("close")).toEqual([["其他/绪论.md"]]);
});

/** 文件操作期间避免重复关闭，仍显示现有标签以保留上下文。 */
test("忙碌时禁用点击与键盘关闭", async () => {
  const wrapper = tabs();
  await wrapper.setProps({ busy: true });
  expect(wrapper.findAll("button").every((button) => button.attributes("disabled") !== undefined)).toBe(true);
  await wrapper.findAll('[role="tab"]')[0].trigger("keydown", { key: "Delete" });
  await wrapper.findAll(".note-tab")[0].trigger("auxclick", { button: 1 });
  expect(wrapper.emitted("close")).toBeUndefined();
});

/** 只滚动标签容器，不以 scrollIntoView 拉动整页或正文。 */
test("滚轮横向移动溢出标签，激活标签自动进入可视范围", async () => {
  const wrapper = tabs();
  await nextTick();
  const strip = wrapper.get('[role="tablist"]').element as HTMLElement;
  Object.defineProperty(strip, "clientWidth", { value: 180 });
  Object.defineProperty(strip, "scrollWidth", { value: 540 });
  const last = wrapper.findAll(".note-tab")[2].element as HTMLElement;
  Object.defineProperty(last, "offsetLeft", { value: 360 });
  Object.defineProperty(last, "offsetWidth", { value: 180 });
  await wrapper.get('[role="tablist"]').trigger("wheel", { deltaY: 60, deltaX: 0 });
  expect(strip.scrollLeft).toBe(60);
  await wrapper.setProps({ activePath: "其他/绪论.md" });
  await nextTick();
  expect(strip.scrollLeft).toBe(360);
});

/** 原生滚动条被藏掉后，滚动位置只能由虚拟滚动条表达；thumb 长度与位移都得跟着容器几何走。 */
test("溢出时渲染虚拟滚动条并跟随滚动位置", async () => {
  const wrapper = tabs();
  await nextTick();
  const strip = wrapper.get('[role="tablist"]').element as HTMLElement;
  expect(wrapper.find("[data-virtual-scrollbar]").exists()).toBe(false);
  Object.defineProperty(strip, "clientWidth", { value: 180, configurable: true });
  Object.defineProperty(strip, "scrollWidth", { value: 540, configurable: true });
  // jsdom 不会因为赋值 scrollLeft 而派发滚动事件，这里手动补一次真实浏览器里必然发生的通知。
  strip.dispatchEvent(new Event("scroll"));
  await nextTick();
  const bar = wrapper.get("[data-virtual-scrollbar]");
  expect(bar.attributes("data-thumb-length")).toBe("60"); // 180² / 540
  expect(bar.attributes("data-thumb-offset")).toBe("0");
  strip.scrollLeft = 360;
  strip.dispatchEvent(new Event("scroll"));
  await nextTick();
  expect(wrapper.get("[data-virtual-scrollbar]").attributes("data-thumb-offset")).toBe("120"); // 360/360 × (180-60)
});

/** 操作图标并入有内容的标签行，不再在上方叠加空白工具栏。 */
test("标签和右侧操作共用36像素的一行", () => {
  const wrapper = mount(NoteTabs, { props: { tabs: [{ path: "a.md", title: "a" }], activePath: "a.md" },
    slots: { actions: () => h(NoteToolbar, { inline: true, notePath: "a.md", panelOpen: false }) },
  });
  mounted.push(wrapper);
  expect(wrapper.classes()).toContain("h-9");
  expect(wrapper.get(".tab-actions header").classes()).toContain("relative");
  expect(wrapper.get(".tab-actions header").classes()).not.toContain("absolute");
  expect(wrapper.get(".tab-actions").findAll("button")).toHaveLength(2);
});
