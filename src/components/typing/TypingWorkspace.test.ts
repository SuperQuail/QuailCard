import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import TypingWorkspace from "./TypingWorkspace.vue";
import * as backend from "../../api/backend";
import { applyPracticeSettings } from "../../services/stores/typingStore";
import { resetAudioCache } from "../../services/typingAudio";
import type { ReviewCard } from "../../domain/types";

vi.mock("../../api/backend", () => ({
  getReviewQueue: vi.fn(),
  setPracticeSettings: vi.fn(),
  synthesizeSpeech: vi.fn(),
}));

/** 构造单词卡，字段与后端复习队列一致。 */
function card(id: string, front: string, back: string): ReviewCard {
  return {
    id,
    notePath: "英语/词汇.md",
    sourceRef: "",
    kind: "vocabulary",
    front,
    back,
    detail: "",
    example: "",
    aliases: [],
    rubricPoints: [],
    state: "new",
    version: 1,
  };
}

const cards = [card("c1", "短暂的", "ephemeral"), card("c2", "深入思考", "contemplate")];

let wrapper: ReturnType<typeof mount>;

/** 派发一次全局按键，模拟工作区捕获到的打字。 */
async function key(value: string): Promise<void> {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: value }));
  await flushPromises();
}

/** 依次打出一整串字母。 */
async function typeWord(word: string): Promise<void> {
  for (const char of word) await key(char);
}

beforeEach(() => {
  vi.clearAllMocks();
  applyPracticeSettings({ repeatsPerWord: 1, wordsPerPage: 1 });
  resetAudioCache();
  vi.mocked(backend.getReviewQueue).mockResolvedValue(cards);
  vi.mocked(backend.setPracticeSettings).mockImplementation(async (settings) => settings);
  vi.mocked(backend.synthesizeSpeech).mockResolvedValue("data:audio/mp3;base64,AAAA");
});

afterEach(() => {
  wrapper?.unmount();
  vi.useRealTimers();
});

describe("打字背诵工作区", () => {
  test("准备页显示词数与分页，开始后逐字母红绿判定", async () => {
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    expect(wrapper.text()).toContain("2");
    expect(wrapper.text()).toContain("开始练习");

    await wrapper.get("button.primary-btn").trigger("click");
    expect(wrapper.findAll(".typing-cell")).toHaveLength(2);

    await key("e");
    await key("p");
    await key("x");
    const letters = wrapper.findAll(".typing-cell.is-active .typing-cell-letter");
    expect(letters[0].classes()).toContain("is-correct");
    expect(letters[1].classes()).toContain("is-correct");
    expect(letters[2].classes()).toContain("is-wrong");
  });

  test("打错不允许回退：退格被吞掉并提示一次", async () => {
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    await wrapper.get("button.primary-btn").trigger("click");
    await key("e");
    await key("p");
    await key("Backspace");
    const letters = wrapper.findAll(".typing-cell.is-active .typing-cell-letter");
    expect(letters.filter((letter) => letter.classes().includes("is-correct"))).toHaveLength(2);
    expect(wrapper.text()).toContain("不支持回退");
  });

  test("记忆遍释义与目标模糊，打出的字母原地替换", async () => {
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    await wrapper.get("button.primary-btn").trigger("click");

    await typeWord("ephemeral");
    await key(" ");
    const memory = wrapper.get(".typing-cell.is-active.is-memory");
    expect(memory.find(".is-blurred").exists()).toBe(true);
    expect(memory.findAll(".typing-cell-letter.is-sealed").length).toBeGreaterThan(0);
    expect(wrapper.get(".typing-board").classes()).toContain("is-memory");

    await key("e");
    expect(memory.findAll(".typing-cell-word .is-correct")).toHaveLength(1);
    expect(memory.findAll(".typing-cell-letter.is-sealed")).toHaveLength(8);
    expect(wrapper.find(".typing-cell-typed").exists()).toBe(false);

    await key(" ");
    await flushPromises();
    // 本页打满直接翻到第 2 页，不再停在「本页完成」
    expect(wrapper.text()).toContain("第 2 / 2 页");
    expect(wrapper.findAll(".typing-cell.is-active")).toHaveLength(1);
  });

  test("本页打满直接进入结算，不再有「本页完成」等待页", async () => {
    applyPracticeSettings({ repeatsPerWord: 1, wordsPerPage: 2 });
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    await wrapper.get("button.primary-btn").trigger("click");
    expect(wrapper.findAll(".typing-cell")).toHaveLength(4);

    for (let index = 0; index < 3; index += 1) {
      await key(" ");
    }
    expect(wrapper.text()).not.toContain("本轮打字背诵完成");
    await key(" ");
    expect(wrapper.text()).toContain("本轮打字背诵完成");
  });

  test("练习中按 Esc 或返回会先确认是否放弃", async () => {
    const confirm = vi.fn().mockResolvedValue(false);
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all", confirm } });
    await flushPromises();
    await wrapper.get("button.primary-btn").trigger("click");
    await key("e");

    await key("Escape");
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(wrapper.emitted("close")).toBeUndefined();

    await wrapper.get("header button").trigger("click");
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(wrapper.emitted("close")).toBeUndefined();

    confirm.mockResolvedValue(true);
    await key("Escape");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  test("被遮挡时按键无效，恢复后可以继续", async () => {
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    await wrapper.get("button.primary-btn").trigger("click");
    await wrapper.setProps({ suspended: true });
    await key("e");
    expect(wrapper.findAll(".typing-cell.is-active .typing-cell-letter.is-correct")).toHaveLength(0);
    await wrapper.setProps({ suspended: false });
    await key("e");
    expect(wrapper.findAll(".typing-cell.is-active .typing-cell-letter.is-correct")).toHaveLength(1);
  });

  test("范围里没有单词卡时显示空态", async () => {
    vi.mocked(backend.getReviewQueue).mockResolvedValue([{ ...card("q1", "问题", "答案"), kind: "qa" }]);
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: "英语/词汇.md", scope: "note" } });
    await flushPromises();
    expect(wrapper.text()).toContain("还没有单词卡");
    expect(wrapper.text()).toContain("切换到全部单词卡");
  });

  test("发音预取完成前开始按钮锁定，完成后解锁", async () => {
    const releases: Array<(value: string) => void> = [];
    vi.mocked(backend.synthesizeSpeech).mockImplementation(() => new Promise<string>((resolve) => { releases.push(resolve); }));
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    const start = wrapper.findAll("button").find((button) => button.text().includes("准备发音"));
    expect(start).toBeTruthy();
    expect((start?.element as HTMLButtonElement).disabled).toBe(true);
    for (const release of releases) release("data:audio/mp3;base64,AAAA");
    await flushPromises();
    expect(wrapper.text()).toContain("开始练习");
  });

  test("调整每词遍数会持久化并重建分页预览", async () => {
    wrapper = mount(TypingWorkspace, { props: { title: "打字背诵", notePath: null, scope: "all" } });
    await flushPromises();
    const increase = wrapper.findAll("button[aria-label='增加遍数']");
    await increase[0].trigger("click");
    await flushPromises();
    expect(backend.setPracticeSettings).toHaveBeenCalledWith({ repeatsPerWord: 2, wordsPerPage: 1 });
    const stepperValues = wrapper.findAll("span.w-6").map((span) => span.text());
    expect(stepperValues).toEqual(["2", "1"]);
  });
});
