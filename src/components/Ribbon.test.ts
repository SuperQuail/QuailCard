import { Bot, Brain, Moon, PanelLeftClose, PanelLeftOpen, PenLine, Play, Search, Settings, Sun } from "@lucide/vue";
import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, test } from "vitest";
import Ribbon from "./Ribbon.vue";

const mounted: ReturnType<typeof mount>[] = [];

/** 每个用例独立卸载，避免导航状态残留。 */
afterEach(() => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount();
});

/** 保留真实 Lucide SVG，验证布局调整不替换图标。 */
function mountRibbon() {
  const wrapper = mount(Ribbon, { props: { treeOpen: true, dueCount: 4, dark: false } });
  mounted.push(wrapper);
  return wrapper;
}

/** 导航与主题操作仍由父组件处理。 */
describe("Ribbon 导航布局", () => {
  /** 每个原有入口都必须保留图标并仅发送对应事件。 */
  test("保留全部入口、Lucide 图标和导航事件", async () => {
    const wrapper = mountRibbon();
    const entries = [
      ["收起文件树", "toggle-tree", PanelLeftClose],
      ["搜索（Ctrl+K）", "open-palette", Search],
      ["快速捕获（Ctrl+N）", "open-capture", PenLine],
      ["学习 Agent", "open-agent", Bot],
      ["视频转笔记", "open-video", Play],
      ["复习工作区", "open-review", Brain],
      ["切换主题", "toggle-theme", Moon],
      ["设置", "open-settings", Settings],
    ] as const;
    expect(wrapper.findAll("button")).toHaveLength(entries.length);
    for (const [title, event, icon] of entries) {
      const button = wrapper.get('button[title="' + title + '"]');
      expect(button.findComponent(icon).exists()).toBe(true);
      expect(button.find("svg").exists()).toBe(true);
      expect((button.element as HTMLButtonElement).tabIndex).toBe(0);
      await button.trigger("click");
      expect(wrapper.emitted(event)).toEqual([[]]);
    }
    expect(wrapper.get("nav").classes()).toContain("w-[52px]");
    const bottom = wrapper.get("nav > div.mt-auto");
    expect(bottom.findAll("button").map((button) => button.attributes("title"))).toEqual(["切换主题", "设置"]);
  });

  /** 状态来自 props，按钮图标和复习徽章随父组件更新。 */
  test("保留展开主题切换图标、导航选中态和复习数量", async () => {
    const wrapper = mountRibbon();
    expect(wrapper.get('button[title="复习工作区"] span').text()).toBe("4");
    await wrapper.setProps({ treeOpen: false, dark: true, dueCount: 0, agentOpen: true, reviewOpen: true, videoOpen: true });
    expect(wrapper.findComponent(PanelLeftOpen).exists()).toBe(true);
    expect(wrapper.findComponent(PanelLeftClose).exists()).toBe(false);
    expect(wrapper.findComponent(Sun).exists()).toBe(true);
    expect(wrapper.findComponent(Moon).exists()).toBe(false);
    expect(wrapper.find('button[title="复习工作区"] span').exists()).toBe(false);
    for (const title of ["学习 Agent", "视频转笔记", "复习工作区"]) {
      const button = wrapper.get('button[title="' + title + '"]');
      expect(button.classes()).toContain("active");
      expect(button.attributes("aria-pressed")).toBe("true");
    }
    await wrapper.get('button[title="展开文件树"]').trigger("click");
    expect(wrapper.emitted("toggle-tree")).toEqual([[]]);
    await wrapper.get('button[title="复习工作区"]').trigger("click");
    expect(wrapper.emitted("open-review")).toEqual([[]]);
  });
});
