import { describe, expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import AgentComposer from "./AgentComposer.vue";

/** 仅提供输入区所需状态，发送规则与后端调用保持隔离。 */
function props() {
  return { draft: "学习", selectedPaths: [] as string[], providerId: "", providers: [], notes: [], ready: true, running: false, loading: false, error: "" };
}

describe("Agent 输入区交互", () => {
  test("图片粘贴转发文件，纯文字保留默认行为，只有图片也可以发送和移除", async () => {
    const wrapper = mount(AgentComposer, { props: { ...props(), draft: "" } });
    const file = new File(["image"], "paste.png", { type: "image/png" });
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { files: [file], getData: () => "" } });
    wrapper.get("textarea").element.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(wrapper.emitted("pasteImages")).toEqual([[[file]]]);
    const textEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(textEvent, "clipboardData", { value: { files: [], items: [] } });
    wrapper.get("textarea").element.dispatchEvent(textEvent);
    expect(textEvent.defaultPrevented).toBe(false);
    await wrapper.setProps({ images: [{ name: "paste.png", mimeType: "image/png", dataBase64: "aW1hZ2U=" }] });
    expect(wrapper.get("img").attributes("src")).toBe("data:image/png;base64,aW1hZ2U=");
    await wrapper.get('[aria-label="发送"]').trigger("click");
    expect(wrapper.emitted("send")).toEqual([[""]]);
    await wrapper.get('[aria-label="移除图片 1"]').trigger("click");
    expect(wrapper.emitted("removeImage")).toEqual([[0]]);
    await wrapper.setProps({ readingImages: true });
    expect(wrapper.get('[aria-label="发送"]').attributes("disabled")).toBeDefined();
    wrapper.unmount();
  });
  test("中文候选确认与 Shift+Enter 不发送且不拦截默认行为", () => {
    const wrapper = mount(AgentComposer, { props: props() });
    const textarea = wrapper.get("textarea").element;
    for (const options of [{ isComposing: true }, { shiftKey: true }, { keyCode: 229 }]) {
      const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true, ...options });
      textarea.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(wrapper.emitted("send")).toBeUndefined();
    textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(wrapper.emitted("send")).toEqual([["学习"]]);
  });

  test.each([{ draft: " \n " }, { loading: true }, { ready: false }, { running: true }])("禁止不可发送状态的键盘提交：%j", async state => {
    const wrapper = mount(AgentComposer, { props: { ...props(), ...state } });
    await wrapper.get("textarea").trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("send")).toBeUndefined();
  });

  test("生成中停止以及引用移除均转发独立事件", async () => {
    const wrapper = mount(AgentComposer, { props: { ...props(), running: true, selectedPaths: ["笔记.md"] } });
    expect(wrapper.find("select").exists()).toBe(false);
    await wrapper.get('[aria-label="停止生成"]').trigger("click");
    await wrapper.get('[aria-label="移除引用 笔记.md"]').trigger("click");
    expect(wrapper.emitted("stop")).toHaveLength(1);
    expect(wrapper.emitted("scope")).toEqual([[[]]]);
  });

  test("@ 打开范围选择且输入高度封顶，草稿清空后收缩", async () => {
    const wrapper = mount(AgentComposer, { props: props() });
    const input = wrapper.get("textarea");
    let height = 350;
    Object.defineProperty(input.element, "scrollHeight", { configurable: true, get: () => height });
    await input.setValue("学习@");
    expect(wrapper.emitted("draft")?.[0]).toEqual(["学习@"]);
    expect(wrapper.find('[aria-label="搜索引用笔记"]').exists()).toBe(true);
    expect(input.element.style.height).toBe("200px");
    expect(input.element.style.overflowY).toBe("auto");
    height = 48;
    await wrapper.setProps({ draft: "" });
    expect(input.element.style.height).toBe("48px");
    expect(input.element.style.overflowY).toBe("hidden");
  });
});
