import { afterEach, describe, expect, test, vi } from "vitest";
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import * as model from "../../markdown/model";
import AgentText from "./AgentText.vue";

enableAutoUnmount(afterEach);
/** 还原模块监视器和浏览器 API，避免影响其他组件测试。 */
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

/** 一次调用只推进当前帧，不消费回调新安排的下一帧。 */
function frames() {
  const pending = new Map<number, FrameRequestCallback>();
  let id = 0;
  vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => { pending.set(++id, callback); return id; }));
  vi.stubGlobal("cancelAnimationFrame", vi.fn((key: number) => pending.delete(key)));
  return { pending, async step() {
    const callbacks = [...pending.values()]; pending.clear();
    for (const callback of callbacks) callback(16);
    await nextTick();
  } };
}

// 按调用次数断言结构上限，不依赖机器速度；渲染和音标共享一次解析。
test("长Markdown跨帧积压与输入突发每帧仅解析和通知一次", async () => {
  const clock = frames();
  const parse = vi.spyOn(model, "parseNoteBlocks");
  const wrapper = mount(AgentText, { props: { text: "# 标题\n\n" + "内容😀\n\n".repeat(2000), animate: true } });
  parse.mockClear();
  for (let index = 0; index < 20; index++) await wrapper.setProps({ text: wrapper.props("text") + "追加" });
  expect(parse).not.toHaveBeenCalled(); expect(wrapper.emitted("progress")).toBeUndefined();
  expect(clock.pending.size).toBe(1);
  for (let frame = 1; frame <= 6; frame++) {
    await clock.step();
    expect(parse).toHaveBeenCalledTimes(frame);
    expect(wrapper.emitted("progress")).toHaveLength(frame);
    for (let burst = 0; burst < 3; burst++) await wrapper.setProps({ text: wrapper.props("text") + "尾" });
    expect(parse).toHaveBeenCalledTimes(frame);
    expect(wrapper.emitted("progress")).toHaveLength(frame);
    expect(clock.pending.size).toBe(1);
  }
});

// 前缀清空若在 watcher 中立即发布，会使两个网络块之间额外解析并触发滚动。
test("同帧前缀替换仅解析最终显示前缀，取消不等待下一帧", async () => {
  const clock = frames();
  const parse = vi.spyOn(model, "parseNoteBlocks");
  const wrapper = mount(AgentText, { props: { text: "旧文本", animate: true } });
  await clock.step(); parse.mockClear();
  await wrapper.setProps({ text: "替换" });
  await wrapper.setProps({ text: "新😀文" });
  expect(parse).not.toHaveBeenCalled();
  await clock.step();
  expect(parse).toHaveBeenCalledExactlyOnceWith("新");
  expect(wrapper.text()).toBe("新");
  await wrapper.setProps({ animate: false });
  expect(wrapper.text()).toBe("新😀文");
  expect(clock.pending.size).toBe(0);
  const count = parse.mock.calls.length;
  await clock.step(); expect(parse).toHaveBeenCalledTimes(count);
});

// 块级渲染：标题、列表、表格、删除线与站内跳转都由统一模型驱动。
describe("AgentText 块级渲染", () => {
  test("标题、有序列表、任务项、表格与删除线按模型渲染", () => {
    const text = [
      "#### 四级标题",
      "",
      "1. 有序项",
      "",
      "- [x] 完成项",
      "",
      "| 列一 | 列二 |",
      "| --- | --- |",
      "| 甲 | ~~乙~~ |",
      "",
      "正文 <img src=x>",
    ].join("\n");
    const wrapper = mount(AgentText, { props: { text } });
    expect(wrapper.findAll("div.whitespace-pre-wrap")[0].text()).toBe("四级标题");
    const items = wrapper.findAll("li");
    expect(items.map((item) => item.text())).toEqual(["1.有序项", "☑完成项"]);
    expect(wrapper.findAll("th").map((cell) => cell.text())).toEqual(["列一", "列二"]);
    expect(wrapper.findAll("td").map((cell) => cell.text())).toEqual(["甲", "乙"]);
    expect(wrapper.get("td s").text()).toBe("乙");
    const paragraphs = wrapper.findAll("div.whitespace-pre-wrap");
    expect(paragraphs[paragraphs.length - 1].text()).toBe("正文 <img src=x>");
    expect(wrapper.find("img").exists()).toBe(false);
  });

  test("单行公式用 KaTeX 渲染，其余文字不受影响", () => {
    const wrapper = mount(AgentText, { props: { text: "公式 $x^2$ 结束" } });
    expect(wrapper.find(".qc-math .katex").exists()).toBe(true);
    expect(wrapper.text()).toContain("结束");
  });

  test("只有站内 .md 链接渲染成跳转入口", async () => {
    const wrapper = mount(AgentText, { props: { text: "见 [外链](https://a.com) 与 [笔记](b.md)" } });
    const buttons = wrapper.findAll("button");
    expect(buttons).toHaveLength(1);
    expect(buttons[0].text()).toBe("笔记");
    await buttons[0].trigger("click");
    expect(wrapper.emitted("note")).toEqual([["b.md"]]);
    expect(wrapper.text()).toContain("外链");
  });
});

// 历史直接呈现，既有安全转义和站内笔记动作不受调度改变。
test("历史Markdown即时渲染并保留安全笔记跳转", async () => {
  const clock = frames();
  const wrapper = mount(AgentText, { props: { text: "<img src=x>\n\n[笔记](a.md)" } });
  expect(wrapper.find("img").exists()).toBe(false);
  expect(wrapper.text()).toContain("<img src=x>");
  await wrapper.get("button").trigger("click");
  expect(wrapper.emitted("note")).toEqual([["a.md"]]);
  expect(clock.pending.size).toBe(0);
});
