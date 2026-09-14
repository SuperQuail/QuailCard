import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProviderSummary } from "../../domain/types";
import { deleteProvider, getOpenAiLoginStatus, logoutOpenAi, startOpenAiLogin } from "../../services/stores/providerStore";
import ModelSettings from "./ModelSettings.vue";
import ProviderForm from "./ProviderForm.vue";

vi.mock("../../services/stores/providerStore", () => ({
  deleteProvider: vi.fn(),
  testProvider: vi.fn(),
  getOpenAiLoginStatus: vi.fn(),
  logoutOpenAi: vi.fn(),
  startOpenAiLogin: vi.fn(),
}));
vi.mock("../../api/backend", () => ({ isTauri: () => false }));

/** 单张卡片所需的最小摘要。 */
function provider(overrides: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: "open_code",
    name: "OpenCode Go",
    shortCode: "OG",
    protocol: "OpenAI Compatible",
    model: "deepseek-v4-flash",
    models: [
      { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash", contextWindow: null, maxOutputTokens: null },
      { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro", contextWindow: null, maxOutputTokens: null },
    ],
    baseUrl: "https://opencode.ai/zen/go/v1",
    hasApiKey: true,
    hasCredential: true,
    authType: "api_key",
    oauthAccountId: null,
    providerType: "api",
    supportsVision: false,
    status: "connected",
    ...overrides,
  };
}

/** 卡片头部的编辑按钮在展开后会变成「收起」，因此按两者之一匹配。 */
function editButton(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll("button").find((button) => ["编辑", "收起"].includes(button.text()))!;
}

/** 每个用例隔离服务调用记录与浏览器计时器。 */
beforeEach(() => vi.clearAllMocks());
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("模型设置卡片", () => {
  test("列表移除测试入口，红色文字删除按钮保留二次确认", async () => {
    vi.mocked(deleteProvider).mockResolvedValue(undefined);
    const wrapper = mount(ModelSettings, {
      props: { providers: [provider()], activeProviderId: "open_code" },
    });
    const header = wrapper.get("header");
    expect(header.find('[title="测试连接"]').exists()).toBe(false);
    const remove = header.get('[title="删除"]');
    expect(remove.text()).toBe("删除");
    expect(remove.classes()).toEqual(expect.arrayContaining(["ghost-btn", "!text-danger"]));
    expect(remove.find("svg").exists()).toBe(false);
    await remove.trigger("click");
    expect(remove.text()).toBe("确认删除");
    expect(deleteProvider).not.toHaveBeenCalled();
    await remove.trigger("click");
    await flushPromises();
    expect(deleteProvider).toHaveBeenCalledExactlyOnceWith("open_code");
    await editButton(wrapper).trigger("click");
    expect(wrapper.getComponent(ProviderForm).text()).toContain("测试");
    expect(wrapper.getComponent(ProviderForm).find('input[type="password"]').exists()).toBe(true);
    wrapper.unmount();
  });

  test("OpenAI 登录仅在编辑表单内显示，授权结果也显示在表单内", async () => {
    vi.useFakeTimers();
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    vi.mocked(startOpenAiLogin).mockResolvedValue({
      attemptId: "login-1", mode: "browser", url: "https://auth.openai.com/login", userCode: null,
    });
    vi.mocked(getOpenAiLoginStatus).mockResolvedValue({ status: "success", message: "", provider: null });
    const wrapper = mount(ModelSettings, {
      props: {
        providers: [provider({ id: "openai_subscription", providerType: "openai_subscription", hasCredential: false })],
        activeProviderId: "openai_subscription",
      },
    });
    expect(wrapper.text()).not.toContain("登录 OpenAI");
    await editButton(wrapper).trigger("click");
    const form = wrapper.getComponent(ProviderForm);
    expect(wrapper.get("header").text()).not.toContain("登录");
    expect(form.find('input[type="password"]').exists()).toBe(false);
    const login = form.findAll("button").find((button) => button.text() === "登录 OpenAI")!;
    await login.trigger("click");
    await flushPromises();
    expect(startOpenAiLogin).toHaveBeenCalledExactlyOnceWith("openai_subscription", "browser");
    expect(open).toHaveBeenCalledWith("https://auth.openai.com/login", "_blank");
    expect(login.attributes("disabled")).toBeDefined();
    await vi.advanceTimersByTimeAsync(2000);
    expect(form.text()).toContain("登录成功");
    await editButton(wrapper).trigger("click");
    expect(wrapper.text()).not.toContain("登录成功");
    wrapper.unmount();
  });

  test("已登录的 OpenAI 在编辑表单内提供注销", async () => {
    const subscription = provider({ id: "openai_subscription", providerType: "openai_subscription" });
    vi.mocked(logoutOpenAi).mockResolvedValue({ ...subscription, hasCredential: false });
    const wrapper = mount(ModelSettings, {
      props: { providers: [subscription], activeProviderId: subscription.id },
    });
    expect(wrapper.text()).not.toContain("注销登录");
    await editButton(wrapper).trigger("click");
    const form = wrapper.getComponent(ProviderForm);
    await form.findAll("button").find((button) => button.text() === "注销登录")!.trigger("click");
    await flushPromises();
    expect(logoutOpenAi).toHaveBeenCalledExactlyOnceWith(subscription.id);
    expect(form.text()).toContain("已注销");
    expect(wrapper.get("header").text()).not.toContain("注销");
    wrapper.unmount();
  });

  test("卡片摘要显示当前模型与目录数量", () => {
    const wrapper = mount(ModelSettings, {
      props: { providers: [provider()], activeProviderId: "open_code" },
    });
    expect(wrapper.text()).toContain("deepseek-v4-flash");
    expect(wrapper.text()).toContain("等 2 个模型");
    expect(wrapper.text()).toContain("已连接");
    wrapper.unmount();
  });

  test("点击编辑就地展开表单，收起后回到摘要", async () => {
    const wrapper = mount(ModelSettings, {
      props: { providers: [provider()], activeProviderId: "open_code" },
    });
    expect(wrapper.find('input[placeholder^="模型 ID"]').exists()).toBe(false);
    await editButton(wrapper).trigger("click");
    expect(wrapper.findAll('input[placeholder^="模型 ID"]')).toHaveLength(2);
    await editButton(wrapper).trigger("click");
    expect(wrapper.find('input[placeholder^="模型 ID"]').exists()).toBe(false);
    wrapper.unmount();
  });

  test("新增供应商时表单独立出现且基础配置直接可见", async () => {
    const wrapper = mount(ModelSettings, { props: { providers: [], activeProviderId: "" } });
    const create = wrapper.findAll("button").find((button) => button.text().includes("新增供应商"))!;
    await create.trigger("click");
    expect(wrapper.find('input[placeholder^="名称"]').exists()).toBe(true);
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(true);
    wrapper.unmount();
  });
});
