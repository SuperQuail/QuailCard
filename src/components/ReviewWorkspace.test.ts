import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { ref } from "vue";
import ReviewWorkspace from "./ReviewWorkspace.vue";
import * as backend from "../api/backend";
import { getReviewQueue } from "../services/stores/reviewStore";
import type { ReviewCard } from "../domain/types";

vi.mock("../api/backend", () => ({ submitReview: vi.fn(), evaluateAnswer: vi.fn() }));
vi.mock("../services/stores/reviewStore", () => ({
  aiGradingEnabled: ref(false), getReviewQueue: vi.fn(), refreshStats: vi.fn(),
  evaluateAnswer: vi.fn(), checkDictation: vi.fn(), synthesizeSpeech: vi.fn(),
}));
const card: ReviewCard = { id: "one", notePath: "a.md", sourceRef: "", kind: "qa", front: "问题", back: "参考内容", detail: "", example: "", aliases: [], rubricPoints: [], state: "new", version: 1 };
let wrapper: ReturnType<typeof mount>;
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getReviewQueue).mockResolvedValue([card, { ...card, id: "two", front: "第二题" }]);
  vi.mocked(backend.submitReview).mockResolvedValue({} as never);
});
afterEach(() => wrapper?.unmount());

/** 模拟其他工作区派发的按键，确保隐藏复习不再响应全局事件。 */
async function key(value: string): Promise<void> {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: value }));
  await flushPromises();
}

describe("复习工作区", () => {
  test("切走保留答案和队列，暂停评分快捷键，返回继续原进度", async () => {
    wrapper = mount(ReviewWorkspace, { props: { title: "今日复习", notePath: null, includeAll: false } });
    await flushPromises();
    expect(wrapper.element.tagName).toBe("MAIN");
    expect(wrapper.classes()).not.toContain("fixed");
    await key(" ");
    expect(wrapper.text()).toContain("参考内容");
    await wrapper.setProps({ suspended: true });
    await key("3");
    await key("Escape");
    expect(backend.submitReview).not.toHaveBeenCalled();
    expect(wrapper.emitted("close")).toBeUndefined();
    await wrapper.setProps({ suspended: false });
    expect(wrapper.text()).toContain("参考内容");
    await key("3");
    expect(backend.submitReview).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("第二题");
    expect(getReviewQueue).toHaveBeenCalledTimes(1);
  });
  test("听写草稿切走后仍保留，空格不会在后台提交", async () => {
    vi.mocked(getReviewQueue).mockResolvedValue([{ ...card, kind: "vocabulary" }]);
    wrapper = mount(ReviewWorkspace, { props: { title: "听写", notePath: "a.md", includeAll: true } });
    await flushPromises();
    await wrapper.get("input").setValue("draft");
    await wrapper.setProps({ suspended: true });
    await key(" ");
    await wrapper.setProps({ suspended: false });
    expect((wrapper.get("input").element as HTMLInputElement).value).toBe("draft");
    expect(wrapper.text()).not.toContain("正确答案");
  });
});
