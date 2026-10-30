import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, describe, expect, test } from "vitest";
import ImageContextMenu from "./ImageContextMenu.vue";
import type { ImageMenuRequest } from "../domain/imageMenu";

/** 菜单请求夹具：坐标使用非零值，用于确认组件直接采用 request.x / request.y。 */
const request: ImageMenuRequest = {
  notePath: "英语/单词.md",
  source: "../attachments/pic.png",
  x: 120,
  y: 64,
};

/** 已挂载组件：Teleport 内容挂在 body 上，测试结束必须卸载清理，否则会污染后续用例。 */
const mounted: ReturnType<typeof mount>[] = [];

afterEach(() => {
  for (const wrapper of mounted.splice(0)) {
    wrapper.unmount();
  }
});

/** 挂载菜单；attachTo body 是为了让 Teleport 内容与事件都在真实文档里。 */
function mountMenu(props: { request?: ImageMenuRequest; busy?: boolean } = {}) {
  const wrapper = mount(ImageContextMenu, {
    props: { request, busy: false, ...props },
    attachTo: document.body,
  });
  mounted.push(wrapper);
  return wrapper;
}

/** 按文案取菜单项按钮：按钮同时带 role，用文案定位比类名稳定。 */
function menuItem(label: string): HTMLButtonElement {
  const button = [...document.body.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')]
    .find((candidate) => candidate.textContent?.trim() === label);
  if (!button) throw new Error(`未找到菜单项：${label}`);
  return button;
}

describe("ImageContextMenu 展示", () => {
  test("渲染三项桌面惯用语动作，坐标直接取自请求", () => {
    mountMenu();
    expect([...document.body.querySelectorAll('[role="menuitem"]')].map((item) => item.textContent?.trim()))
      .toEqual(["复制图片", "图片另存为…", "在文件夹中显示"]);
    const panel = document.body.querySelector('[role="menu"]') as HTMLElement;
    expect(panel.getAttribute("style")).toContain("left: 120px");
    expect(panel.getAttribute("style")).toContain("top: 64px");
  });

  test("只有三项菜单项：不设标题行、不配图标，避免被读成弹窗", () => {
    mountMenu();
    const panel = document.body.querySelector('[role="menu"]') as HTMLElement;
    expect(panel.children).toHaveLength(3);
    expect(panel.querySelectorAll("svg")).toHaveLength(0);
  });
});

describe("ImageContextMenu 交互", () => {
  test("点击三项分别上报 copy / save / reveal", async () => {
    const wrapper = mountMenu();
    menuItem("复制图片").click();
    menuItem("图片另存为…").click();
    menuItem("在文件夹中显示").click();
    await nextTick();
    expect(wrapper.emitted("copy")).toEqual([[]]);
    expect(wrapper.emitted("save")).toEqual([[]]);
    expect(wrapper.emitted("reveal")).toEqual([[]]);
    expect(wrapper.emitted("close")).toBeUndefined();
  });

  test("Esc 关闭菜单", async () => {
    const wrapper = mountMenu();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await nextTick();
    expect(wrapper.emitted("close")).toEqual([[]]);
  });

  test("点击遮罩关闭菜单", async () => {
    const wrapper = mountMenu();
    const backdrop = document.body.querySelector(".z-70") as HTMLElement;
    backdrop.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await nextTick();
    expect(wrapper.emitted("close")).toEqual([[]]);
  });

  test("卸载后不再响应 Esc，避免遗留监听误关后续浮层", async () => {
    const wrapper = mountMenu();
    wrapper.unmount();
    mounted.splice(0);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await nextTick();
    expect(wrapper.emitted("close")).toBeUndefined();
  });
});

describe("ImageContextMenu 忙碌状态", () => {
  test("busy 时三项禁用，动作进行中重复点击不会重复触发", () => {
    mountMenu({ busy: true });
    const items = [...document.body.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    expect(items).toHaveLength(3);
    for (const item of items) {
      expect(item.disabled).toBe(true);
    }
  });

  test("busy 时仍可点外部关闭，不把用户困在菜单里", async () => {
    const wrapper = mountMenu({ busy: true });
    const backdrop = document.body.querySelector(".z-70") as HTMLElement;
    backdrop.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await nextTick();
    expect(wrapper.emitted("close")).toEqual([[]]);
  });
});
