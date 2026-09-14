import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import type { VideoTaskHistory } from "../../domain/video";
import VideoHistoryPanel from "./VideoHistoryPanel.vue";

/** 历史项只提供面板需要的字段，避免用例依赖后端完整形状。 */
function item(overrides: Partial<VideoTaskHistory> = {}): VideoTaskHistory {
  return {
    taskId: "t1", url: "https://www.bilibili.com/video/BVfixture", title: "测试视频", state: "completed",
    updatedAt: 0, notePath: null, error: null, pages: [1], quality: 32, ...overrides,
  };
}

/** 面板只接收历史项与两个开关，不读取全局任务状态。 */
function mountPanel(props: Record<string, unknown> = {}) {
  return mount(VideoHistoryPanel, { props: { items: [], error: "", busy: false, ...props } });
}

describe("任务历史面板", () => {
  test("说明只显示手动任务，空列表给出占位文案", () => {
    const wrapper = mountPanel();
    expect(wrapper.text()).toContain("仅显示你手动发起的任务");
    expect(wrapper.text()).toContain("暂无任务记录");
    wrapper.unmount();
  });

  test("状态按本地文案展示，未知状态原样保留", () => {
    const wrapper = mountPanel({ items: [item(), item({ taskId: "t2", title: "", state: "future" })] });
    const rows = wrapper.findAll("li");
    expect(rows[0]!.text()).toContain("测试视频 · 已完成");
    // 标题为空时回退到链接，未知状态照原样输出，便于诊断后端新状态。
    expect(rows[1]!.text()).toContain("https://www.bilibili.com/video/BVfixture · future");
    wrapper.unmount();
  });

  test("失败原因作为悬停说明，打开笔记只在有落盘路径时出现", () => {
    const wrapper = mountPanel({ items: [item({ state: "failed", error: "模型请求超时" })] });
    const row = wrapper.findAll("li")[0]!;
    expect(row.get("span").attributes("title")).toBe("模型请求超时");
    expect(row.findAll("button").map(button => button.text())).toEqual(["恢复参数"]);
    wrapper.unmount();
  });

  test("恢复参数与打开笔记都上抛给父级", async () => {
    const history = item({ notePath: "视频笔记/测试视频.md" });
    const wrapper = mountPanel({ items: [history] });
    const buttons = wrapper.findAll("button");
    await buttons.find(button => button.text() === "打开笔记")!.trigger("click");
    await buttons.find(button => button.text() === "恢复参数")!.trigger("click");
    expect(wrapper.emitted("open-note")).toEqual([["视频笔记/测试视频.md"]]);
    expect(wrapper.emitted("restore")).toEqual([[history]]);
    expect(wrapper.emitted("refresh")).toBeUndefined();
    wrapper.unmount();
  });

  test("解析或启动期间禁用恢复，展开面板时请求刷新", async () => {
    const wrapper = mountPanel({ items: [item()], busy: true });
    const restore = wrapper.findAll("button").find(button => button.text() === "恢复参数")!;
    expect(restore.attributes("disabled")).toBeDefined();
    await restore.trigger("click");
    expect(wrapper.emitted("restore")).toBeUndefined();
    await wrapper.get("summary").trigger("click");
    expect(wrapper.emitted("refresh")).toEqual([[]]);
    wrapper.unmount();
  });

  test("加载失败只提示，不影响已有记录可读", () => {
    const wrapper = mountPanel({ items: [item()], error: "读取历史失败" });
    expect(wrapper.get('[role="status"]').text()).toBe("读取历史失败");
    expect(wrapper.text()).toContain("测试视频 · 已完成");
    wrapper.unmount();
  });
});
