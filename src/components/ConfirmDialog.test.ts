import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, describe, expect, test } from "vitest";
import ConfirmDialog from "./ConfirmDialog.vue";

/** 已挂载的确认框：监听挂在 window 上，测试后必须卸载。 */
const mounted: Array<ReturnType<typeof mount>> = [];

/** 挂到 document 上，焦点与键盘行为才与真实环境一致。 */
function mountDialog(overrides: Record<string, unknown> = {}): ReturnType<typeof mount> {
  const wrapper = mount(ConfirmDialog, {
    props: {
      open: true,
      title: "永久删除",
      body: "即将永久删除 2 项。此操作不可恢复。",
      confirmLabel: "永久删除",
      danger: true,
      ...overrides,
    },
    attachTo: document.body,
  });
  mounted.push(wrapper);
  return wrapper;
}

afterEach(() => {
  for (const wrapper of mounted.splice(0)) {
    wrapper.unmount();
  }
});

describe("ConfirmDialog 渲染与无障碍", () => {
  test("渲染标题正文与两个按钮，对话框有可访问名称", () => {
    const wrapper = mountDialog();
    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.attributes("aria-modal")).toBe("true");
    const labelledBy = dialog.attributes("aria-labelledby");
    expect(labelledBy).toBeTruthy();
    expect(wrapper.get(`#${labelledBy}`).text()).toBe("永久删除");
    expect(wrapper.text()).toContain("即将永久删除 2 项。此操作不可恢复。");
    expect(wrapper.findAll("button").map((button) => button.text())).toEqual(["取消", "永久删除"]);
  });

  test("默认焦点落在取消按钮，避免误按回车触发危险操作", async () => {
    mountDialog();
    await nextTick();
    expect((document.activeElement as HTMLElement | null)?.textContent?.trim()).toBe("取消");
  });

  test("关闭时不渲染任何内容", () => {
    const wrapper = mountDialog({ open: false });
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect(wrapper.text()).toBe("");
  });

  test("danger 为 true 时确认按钮带危险样式类", () => {
    const wrapper = mountDialog();
    const confirm = wrapper.findAll("button").find((button) => button.text() === "永久删除")!;
    expect(confirm.classes()).toContain("danger-btn");
  });

  test("普通确认不带危险样式类", () => {
    const wrapper = mountDialog({ danger: false });
    const confirm = wrapper.findAll("button").find((button) => button.text() === "永久删除")!;
    expect(confirm.classes()).not.toContain("danger-btn");
  });
});

/** 在真实焦点元素上派发回车：事件冒泡到 window，目标即键盘用户当前按着的元素。 */
async function pressEnterOn(element: Element): Promise<void> {
  element.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await nextTick();
}

describe("ConfirmDialog 键盘与点击", () => {
  test("Esc 取消事件只发一次", async () => {
    const wrapper = mountDialog();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await nextTick();
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("confirm")).toBeUndefined();
  });

  test("默认焦点在取消按钮，此时按 Enter 只取消、不确认", async () => {
    const wrapper = mountDialog();
    await nextTick();
    const focused = document.activeElement as HTMLElement | null;
    expect(focused?.textContent?.trim()).toBe("取消");
    await pressEnterOn(focused!);
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("confirm")).toBeUndefined();
  });

  test("Tab 聚焦确认按钮后按 Enter 才确认", async () => {
    const wrapper = mountDialog();
    const confirmButton = wrapper.findAll("button").find((button) => button.text() === "永久删除")!;
    (confirmButton.element as HTMLButtonElement).focus();
    expect((document.activeElement as HTMLElement).textContent?.trim()).toBe("永久删除");
    await pressEnterOn(document.activeElement!);
    expect(wrapper.emitted("confirm")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toBeUndefined();
  });

  test("焦点不在任何按钮上时 Enter 按确认处理", async () => {
    const wrapper = mountDialog();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await nextTick();
    expect(wrapper.emitted("confirm")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toBeUndefined();
  });

  test("点击取消与确认按钮分别派发对应事件", async () => {
    const wrapper = mountDialog();
    const buttons = wrapper.findAll("button");
    await buttons[0].trigger("click");
    await buttons[1].trigger("click");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("confirm")).toHaveLength(1);
  });

  test("关闭后按键盘不再派发事件", async () => {
    const wrapper = mountDialog({ open: false });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await nextTick();
    expect(wrapper.emitted("confirm")).toBeUndefined();
  });

  test("卸载后移除键盘监听", async () => {
    const wrapper = mountDialog();
    wrapper.unmount();
    mounted.splice(mounted.indexOf(wrapper), 1);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await nextTick();
    expect(wrapper.emitted("confirm")).toBeUndefined();
  });
});
