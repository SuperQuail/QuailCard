import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import NoteSidePanel from "./NoteSidePanel.vue";
import { sidePanelTab } from "../../services/stores/uiStore";
import type { Backlink, NoteCard } from "../../domain/types";

/** 构造测试用卡片：只填展示需要的字段。 */
function card(id: string): NoteCard {
  return {
    id, notePath: "a.md", sourceRef: id, source: null, kind: "qa", front: "问", back: "答",
    detail: "", example: "", aliases: [], rubricPoints: [], position: 0,
    schedulerPhase: "new", dueAt: 0, intervalDays: 0, totalReviews: 0, version: 1,
  };
}

/** 三个面板都替换为桩，只验证栏位切换与事件透传。 */
const stubs = { CardPanel: true, OutlinePanel: true, BacklinksPanel: true };

/** 挂载右栏。 */
function mountPanel(backlinks: Backlink[] = []) {
  return mount(NoteSidePanel, {
    props: { cards: [card("c1")], content: "# 标题\n正文", activeCardId: null, activeHeading: null, backlinks },
    global: { stubs },
  });
}

describe("右栏页签", () => {
  test("默认显示卡片面板", () => {
    sidePanelTab.value = "cards";
    const wrapper = mountPanel();
    expect(wrapper.find("card-panel-stub").exists()).toBe(true);
    wrapper.unmount();
  });

  test("切到大纲时显示大纲面板并回写 store", async () => {
    sidePanelTab.value = "cards";
    const wrapper = mountPanel();
    await wrapper.get('[role="tab"][aria-selected="false"]').trigger("click");
    expect(sidePanelTab.value).toBe("outline");
    expect(wrapper.find("outline-panel-stub").exists()).toBe(true);
    wrapper.unmount();
  });

  test("大纲跳转与反链打开都向上冒泡", async () => {
    sidePanelTab.value = "outline";
    const wrapper = mountPanel([{ sourcePath: "b.md", sourceTitle: "b", line: 3, snippet: "见 [[a]]" }]);
    wrapper.findComponent({ name: "OutlinePanel" }).vm.$emit("jump", 12);
    expect(wrapper.emitted("jump")).toEqual([[12]]);
    sidePanelTab.value = "backlinks";
    await wrapper.vm.$nextTick();
    wrapper.findComponent({ name: "BacklinksPanel" }).vm.$emit("open", "b.md", 3);
    expect(wrapper.emitted("open-backlink")).toEqual([["b.md", 3]]);
    wrapper.unmount();
  });
});
