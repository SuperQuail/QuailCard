import type { ConnectionTestResult, OpenAiLoginStart, OpenAiLoginStatus, ProviderInput, ProviderSummary } from "../../domain/types";
import { mockActiveProviderId, mockProviders, setActiveProviderId } from "./state";

/**
 * 供应商域演示处理：列表、保存、删除、连接测试、OpenAI 登录与语音合成。
 *
 * 说明：全部为内存演示，不发起真实网络请求、不保存任何真实凭据。
 * 禁止在本文件中访问真实 API Key 或实现真实 OAuth 流程。
 */

/** 查询供应商列表的副本，避免调用方直接改动内部状态。 */
export function listProviders(): ProviderSummary[] {
  return [...mockProviders];
}

/** 设置活动供应商 id（演示级，不做校验）。 */
export function setActiveProvider(providerId: string): void {
  setActiveProviderId(providerId);
}

/** 保存/新建供应商（演示级）。 */
export function saveProvider(input: ProviderInput): ProviderSummary {
  const existing = input.id ? mockProviders.find((item) => item.id === input.id) : undefined;
  if (existing) {
    existing.name = input.name;
    existing.shortCode = input.name.slice(0, 2).toUpperCase();
    existing.protocol = input.protocol ?? existing.protocol;
    existing.model = input.model;
    existing.baseUrl = input.baseUrl ?? existing.baseUrl;
    // 仅演示：只要传了 apiKey 就标记为“已连接”，不做真实校验。
    if (input.apiKey) {
      existing.hasApiKey = true;
      existing.hasCredential = true;
      existing.authType = "api_key";
      existing.status = "connected";
    }
    return { ...existing };
  }
  const provider: ProviderSummary = {
    id: `provider-${input.name}`,
    name: input.name,
    shortCode: input.name.slice(0, 2).toUpperCase(),
    protocol: input.protocol ?? "OpenAI Compatible",
    model: input.model,
    baseUrl: input.baseUrl ?? "https://api.openai.com/v1",
    hasApiKey: Boolean(input.apiKey),
    hasCredential: Boolean(input.apiKey),
    authType: input.apiKey ? "api_key" : null,
    oauthAccountId: null,
    providerType: "api",
    supportsVision: true,
    status: input.apiKey ? "connected" : "untested",
  };
  mockProviders.push(provider);
  return { ...provider };
}

/** 删除供应商；若删除的是活动供应商，则回退到列表第一个。 */
export function deleteProvider(providerId: string): void {
  const index = mockProviders.findIndex((item) => item.id === providerId);
  if (index >= 0) {
    mockProviders.splice(index, 1);
  }
  if (mockActiveProviderId === providerId) {
    setActiveProviderId(mockProviders[0]?.id ?? "");
  }
}

/** 连接测试（演示级：固定延迟 518ms，直接标记为 connected）。 */
export function testProvider(input: ProviderInput): ConnectionTestResult {
  const provider = input.id ? mockProviders.find((item) => item.id === input.id) : undefined;
  if (provider) {
    provider.status = "connected";
    return { latencyMs: 518, provider: { ...provider } };
  }
  return { latencyMs: 518, provider: null };
}

/** 启动 OpenAI 登录（演示级：仅返回假 URL，不做真实跳转与轮询）。 */
export function startOpenAiLogin(): OpenAiLoginStart {
  return { attemptId: "mock-attempt", mode: "browser", url: "https://auth.openai.com/", userCode: null };
}

/** 查询登录状态（演示级：恒定返回成功）。 */
export function getOpenAiLoginStatus(): OpenAiLoginStatus {
  return { status: "success", message: "登录成功（演示）", provider: null };
}

/** 取消登录（演示级）。 */
export function cancelOpenAiLogin(): OpenAiLoginStatus {
  return { status: "cancelled", message: "已取消", provider: null };
}

/** 注销登录（演示级：仅返回 null，表示无真实会话需销毁）。 */
export function logoutOpenAi(): null {
  return null;
}

/** 合成语音（演示级：不产生音频，返回空串表示无音频）。 */
export function synthesizeSpeech(): string {
  return "";
}
