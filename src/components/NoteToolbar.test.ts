import { BookOpen, Pencil, PanelRightOpen, PanelRightClose } from "@lucide/vue";
import { mount } from "@vue/test-utils";
import { afterEach, expect, test } from "vitest";
import NoteToolbar from "./NoteToolbar.vue";

const mounted: ReturnType<typeof mount>[] = [];
/** 每个测试释放组件，避免共享响应式与 DOM 状态。 */
afterEach(() => { mounted.splice(0).forEach((wrapper) => wrapper.unmount()); });
/** 使用真实工具栏组件验证展示与事件，不引入应用业务状态。 */
function toolbar() {
  const wrapper = mount(NoteToolbar, { props: {
    notePath: "408 数据结构/00 学习指南.md", panelOpen: false,
  } });
  mounted.push(wrapper);
  return wrapper;
}

/** 顶部只显示操作，不再承载路径、字数或保存状态。 */
test("顶部不显示路径和保存提示", () => {
  const wrapper = toolbar();
  expect(wrapper.find("nav").exists()).toBe(false);
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
  expect(wrapper.text()).toBe("");
  expect(wrapper.findAll("button")).toHaveLength(2);
});

/** 防止只删文字却仍留下占整行高度的空工具栏。 */
test("操作区绝对定位，不占顶栏高度且没有横向分隔线", () => {
  const wrapper = toolbar();
  const header = wrapper.get("header");
  expect(header.classes()).toContain("absolute");
  expect(header.classes()).toContain("right-6");
  expect(header.classes()).toContain("top-2");
  expect(header.classes()).not.toContain("min-h-14");
  expect(header.classes()).not.toContain("border-b");
  expect(header.classes()).not.toContain("w-full");
  expect(header.classes()).not.toContain("bg-bg-paper");
});

/** 右上角只放统一尺寸的图标操作，阅读动作不再有常驻背景或文字标签。 */
test("操作组仅含阅读和侧栏图标，侧栏状态只强调颜色", async () => {
  const wrapper = toolbar();
  const actions = wrapper.get('[role="group"][aria-label="笔记操作"]');
  expect(actions.classes()).toContain("gap-1");
  expect(actions.findAll("button")).toHaveLength(2);
  expect(actions.text()).toBe("");
  for (const button of actions.findAll("button")) {
    expect(button.classes()).toContain("icon-btn");
    expect(button.classes()).not.toContain("border");
    expect(button.classes()).not.toContain("active");
    expect(button.get("svg").attributes("width")).toBe("16");
    expect(button.attributes("aria-label")).toBeTruthy();
    expect(button.attributes("title")).toBeTruthy();
  }
  expect(actions.findAll("button")[1].findComponent(PanelRightOpen).exists()).toBe(true);
  await wrapper.setProps({ reading: true, panelOpen: true });
  const buttons = actions.findAll("button");
  expect(actions.text()).toBe("");
  expect(buttons[0].classes()).toEqual(["icon-btn"]);
  expect(buttons[1].classes()).toContain("is-open");
  expect(buttons[1].classes()).not.toContain("active");
  expect(buttons[1].findComponent(PanelRightClose).exists()).toBe(true);
});

/** 阅读按钮只发送切换事件，实际模式由编辑器反馈。 */
test("切换阅读和编辑入口，空工作区隐藏按钮", async () => {
  const wrapper = toolbar();
  const button = wrapper.get('[aria-label="切换到阅读模式"]');
  expect(button.attributes("title")).toBe("切换到阅读模式");
  expect(button.findComponent(BookOpen).exists()).toBe(true);
  await button.trigger("click");
  expect(wrapper.emitted("toggle-reading")).toEqual([[]]);
  await wrapper.setProps({ reading: true });
  const edit = wrapper.get('[aria-label="切换到编辑模式"]');
  expect(edit.attributes("title")).toBe("切换到编辑模式");
  expect(edit.findComponent(Pencil).exists()).toBe(true);
  expect(edit.attributes("aria-pressed")).toBeUndefined();
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(wrapper.emitted("toggle-reading")).toHaveLength(2);
  await wrapper.setProps({ notePath: undefined });
  expect(wrapper.find('[aria-label="切换到编辑模式"]').exists()).toBe(false);
});

/** 收起和展开均由父层驱动，工具栏只转发操作。 */
test("右侧栏开关发送事件并反映父层展开状态", async () => {
  const wrapper = toolbar();
  await wrapper.get('[aria-label="打开右侧栏"]').trigger('click');
  expect(wrapper.emitted('toggle-panel')).toEqual([[]]);
  await wrapper.setProps({ panelOpen: true });
  const close = wrapper.get('[aria-label="收起右侧栏"]');
  expect(close.attributes('aria-expanded')).toBe('true');
  await close.trigger('click');
  expect(wrapper.emitted('toggle-panel')).toHaveLength(2);
});

/** 附件等操作错误仍保留独立错误提示，不混入已移除的保存状态。 */
test("保留实际编辑器操作错误", async () => {
  const wrapper = toolbar();
  await wrapper.setProps({ editorError: "图片加载失败" });
  expect(wrapper.get('[role="alert"]').text()).toBe("图片加载失败");
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
});

/** 空工作区仅保留面板开关，不显示笔记或保存信息。 */
test("空工作区不显示字数和保存成功", async () => {
  const wrapper = toolbar();
  await wrapper.setProps({ notePath: undefined });
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
  expect(wrapper.text()).not.toContain('字数');
  expect(wrapper.text()).toBe('');
  expect(wrapper.find('[aria-label="打开右侧栏"]').exists()).toBe(true);
});
