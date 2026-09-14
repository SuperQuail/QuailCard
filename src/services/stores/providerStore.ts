import { ref } from "vue";
import * as backend from "../../api/backend";
import type {
  ConnectionTestResult,
  OpenAiLoginStart,
  OpenAiLoginStatus,
  ProviderInput,
  ProviderSummary,
} from "../../domain/types";

/**
 * 供应商域 store：AI 供应商列表、活动供应商与 OpenAI 订阅登录。
 * 自成一体，不依赖其他 store。
 */
export const providers = ref<ProviderSummary[]>([]);
export const activeProviderId = ref("");

/** 切换活动供应商。 */
export async function setActiveProvider(providerId: string): Promise<void> {
  await backend.setActiveProvider(providerId);
  activeProviderId.value = providerId;
}

/** 将供应商摘要写入共享列表（新增或原地更新）。 */
export function upsertProvider(provider: ProviderSummary): void {
  const index = providers.value.findIndex((item) => item.id === provider.id);
  if (index >= 0) {
    providers.value[index] = provider;
  } else {
    providers.value.push(provider);
  }
}

/** 保存供应商并更新列表。 */
export async function saveProvider(input: ProviderInput): Promise<ProviderSummary> {
  const provider = await backend.saveProvider(input);
  upsertProvider(provider);
  return provider;
}

/** 测试供应商连接：成功返回的供应商摘要会回写列表。 */
export async function testProvider(input: ProviderInput): Promise<ConnectionTestResult> {
  const result = await backend.testProvider(input);
  if (result.provider) {
    upsertProvider(result.provider);
  }
  return result;
}

/** 删除供应商：若删除的是活动供应商则回退到列表第一个。 */
export async function deleteProvider(providerId: string): Promise<void> {
  await backend.deleteProvider(providerId);
  providers.value = providers.value.filter((item) => item.id !== providerId);
  if (activeProviderId.value === providerId) {
    activeProviderId.value = providers.value[0]?.id ?? "";
  }
}

/** 启动 OpenAI 登录。 */
export function startOpenAiLogin(providerId: string, mode: "browser" | "device"): Promise<OpenAiLoginStart> {
  return backend.startOpenAiLogin(providerId, mode);
}

/** 查询 OpenAI 登录状态并同步成功结果。 */
export async function getOpenAiLoginStatus(attemptId: string): Promise<OpenAiLoginStatus> {
  const status = await backend.getOpenAiLoginStatus(attemptId);
  if (status.provider) {
    upsertProvider(status.provider);
  }
  return status;
}

/** 注销 OpenAI 登录。 */
export async function logoutOpenAi(providerId: string): Promise<ProviderSummary> {
  const provider = await backend.logoutOpenAi(providerId);
  upsertProvider(provider);
  return provider;
}
