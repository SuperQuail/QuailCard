import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { ProviderSummary } from "../../domain/types";
import { saveProvider } from "../../services/stores/providerStore";
import ProviderForm from "./ProviderForm.vue";

vi.mock("../../services/stores/providerStore", () => ({
  saveProvider: vi.fn(),
  testProvider: vi.fn(),
}));

/** 只构造表单需要的摘要字段，用例可覆盖单项。 */
function provider(overrides: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: "open_code",
    name: "OpenCode Go",
    shortCode: "OG",
    protocol: "OpenAI Compatible",
    model: "deepseek-v4-flash",
    models: [
      { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash", contextWindow: 1048576, maxOutputTokens: null },
      { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro", contextWindow: 1048576, maxOutputTokens: 32768 },
    ],
    baseUrl: "https://opencode.ai/zen/go/v1",
    hasApiKey: true,
    hasCredential: true,
    authType: "api_key",
    oauthAccountId: null,
    providerType: "api",
    supportsVision: true,
    status: "connected",
    ...overrides,
  };
}

/** 视觉开关是表单唯一的复选框，避免用子串选择器误判。 */
function visionBox(wrapper: ReturnType<typeof mount>) {
  return wrapper.get<HTMLInputElement>('input[type="checkbox"]');
}

/** 主操作按钮：保存。 */
function saveButton(wrapper: ReturnType<typeof mount>) {
  return wrapper.get<HTMLButtonElement>("button.primary-btn");
}

/** 点「添加模型」新增一行。 */
async function addModelRow(wrapper: ReturnType<typeof mount>): Promise<void> {
  const button = wrapper.findAll("button").find((item) => item.text().includes("添加模型"));
  await button!.trigger("click");
}

/** 每个用例只关心自己这一次保存，清掉上一个用例留下的调用记录。 */
beforeEach(() => {
  vi.clearAllMocks();
});

describe("供应商模型目录", () => {
  /** 新增与编辑使用同一顺序，图片开关紧跟认证，基础配置不再折叠。 */
  test.each([null, provider()])("基础配置直接展示在密钥上方，图片开关位于密钥下方 %#", (editing) => {
    const wrapper = mount(ProviderForm, { props: { editing } });
    expect(wrapper.text()).not.toContain("自定义设置");
    const labels = wrapper.findAll("label");
    expect(labels.slice(0, 5).map((label) => label.text().split(/\s+/)[0])).toEqual([
      "显示名称", "API", "API", "API", "支持图片输入（视觉）",
    ]);
    expect(labels[0]!.find('input[placeholder^="名称"]').exists()).toBe(true);
    expect(labels[1]!.text()).toBe("API 地址");
    expect(labels[2]!.find("select").exists()).toBe(true);
    expect(labels[3]!.find('input[type="password"]').exists()).toBe(true);
    expect(labels[4]!.find('input[type="checkbox"]').exists()).toBe(true);
    expect(labels.slice(0, 5).every((label) => label.isVisible())).toBe(true);
    wrapper.unmount();
  });

  test("编辑时预填目录，保存提交选中模型与完整目录", async () => {
    vi.mocked(saveProvider).mockResolvedValue(provider());
    const wrapper = mount(ProviderForm, { props: { editing: provider() } });
    const ids = wrapper.findAll<HTMLInputElement>('input[placeholder^="模型 ID"]');
    expect(ids).toHaveLength(2);
    expect(ids[0]!.element.value).toBe("deepseek-v4-flash");
    await wrapper.findAll('input[type="radio"]')[1]!.trigger("change");
    await saveButton(wrapper).trigger("click");
    await vi.waitFor(() => expect(vi.mocked(saveProvider)).toHaveBeenCalled());
    const input = vi.mocked(saveProvider).mock.calls[0]![0];
    expect(input.model).toBe("deepseek-v4-pro");
    expect(input.models).toEqual([
      { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash", contextWindow: 1048576, maxOutputTokens: null },
      { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro", contextWindow: 1048576, maxOutputTokens: 32768 },
    ]);
    wrapper.unmount();
  });

  test("新增行可填上下文窗口与最大输出 token，未填保持空", async () => {
    vi.mocked(saveProvider).mockResolvedValue(provider());
    const wrapper = mount(ProviderForm, { props: { editing: null } });
    await wrapper.get('input[placeholder^="名称"]').setValue("本地模型");
    await wrapper.get('input[placeholder^="模型 ID"]').setValue("local-chat");
    await addModelRow(wrapper);
    const ids = wrapper.findAll<HTMLInputElement>('input[placeholder^="模型 ID"]');
    expect(ids).toHaveLength(2);
    await ids[1]!.setValue("local-reasoner");
    await wrapper.findAll('[title="上下文窗口与输出上限"]')[1]!.trigger("click");
    const contextInput = wrapper.get<HTMLInputElement>('input[placeholder="256K"]');
    const outputInput = wrapper.get<HTMLInputElement>('input[placeholder="32768"]');
    expect(contextInput.element.value).toBe("");
    expect(outputInput.element.value).toBe("");
    await contextInput.setValue("128K");
    await outputInput.setValue("16K");
    await saveButton(wrapper).trigger("click");
    await vi.waitFor(() => expect(vi.mocked(saveProvider)).toHaveBeenCalled());
    const input = vi.mocked(saveProvider).mock.calls[0]![0];
    expect(input.models).toEqual([
      { id: "local-chat", name: "local-chat", contextWindow: null, maxOutputTokens: null },
      { id: "local-reasoner", name: "local-reasoner", contextWindow: 131072, maxOutputTokens: 16384 },
    ]);
    wrapper.unmount();
  });

  test("表单顶部直接显示当前名称，API 地址占位符写出真实默认值", async () => {
    const wrapper = mount(ProviderForm, { props: { editing: provider() } });
    expect(wrapper.get("p.font-semibold").text()).toBe("OpenCode Go");
    expect(wrapper.findAll("button").slice(-3).map((button) => button.text())).toEqual(["测试", "取消", "保存"]);
    wrapper.unmount();
    const creating = mount(ProviderForm, { props: { editing: null } });
    expect(creating.get("p.font-semibold").text()).toBe("新供应商");
    const baseUrl = creating.get<HTMLInputElement>('input[placeholder^="https://api.openai.com/v1"]');
    expect(baseUrl.element.value).toBe("");
    creating.unmount();
  });

  test("模型 ID 为空或重复时禁止保存", async () => {
    vi.mocked(saveProvider).mockResolvedValue(provider());
    const wrapper = mount(ProviderForm, { props: { editing: provider() } });
    expect(saveButton(wrapper).element.disabled).toBe(false);
    await wrapper.findAll<HTMLInputElement>('input[placeholder^="模型 ID"]')[1]!.setValue("deepseek-v4-flash");
    expect(wrapper.text()).toContain("模型 ID 不能重复");
    expect(saveButton(wrapper).element.disabled).toBe(true);
    await wrapper.findAll<HTMLInputElement>('input[placeholder^="模型 ID"]')[1]!.setValue("");
    expect(saveButton(wrapper).element.disabled).toBe(true);
    wrapper.unmount();
  });
});

describe("供应商图片能力", () => {
  test("编辑纯文本供应商时预填为未勾选，保存原样提交", async () => {
    vi.mocked(saveProvider).mockResolvedValue(provider({ supportsVision: false }));
    const wrapper = mount(ProviderForm, { props: { editing: provider({ supportsVision: false }) } });
    expect(visionBox(wrapper).element.checked).toBe(false);
    await saveButton(wrapper).trigger("click");
    await vi.waitFor(() => expect(vi.mocked(saveProvider)).toHaveBeenCalled());
    expect(vi.mocked(saveProvider).mock.calls[0]![0].supportsVision).toBe(false);
    wrapper.unmount();
  });

  test("新增供应商默认勾选，取消后按纯文本提交", async () => {
    vi.mocked(saveProvider).mockResolvedValue(provider({ supportsVision: false }));
    const wrapper = mount(ProviderForm, { props: { editing: null } });
    await wrapper.get('input[placeholder^="名称"]').setValue("纯文本供应商");
    await wrapper.get('input[placeholder^="模型 ID"]').setValue("text-only");
    const box = visionBox(wrapper);
    expect(box.element.checked).toBe(true);
    await box.setValue(false);
    await saveButton(wrapper).trigger("click");
    await vi.waitFor(() => expect(vi.mocked(saveProvider)).toHaveBeenCalled());
    expect(vi.mocked(saveProvider).mock.calls[0]![0].supportsVision).toBe(false);
    wrapper.unmount();
  });
});
