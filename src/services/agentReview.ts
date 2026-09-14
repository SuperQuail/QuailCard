import * as api from "../api/agent";
import type { AgentMessage } from "../domain/agent";
import { createReviewSession, type ReviewSessionFlow, type ReviewSnapshot } from "./reviewSession";

const flows = new Map<string, ReviewSessionFlow>();
/** 每个消息块保持独立复习状态，切换笔记不会重置作答进度。 */
export function agentReviewFlow(sessionId: string, message: AgentMessage): ReviewSessionFlow {
  const key = `${sessionId}:${message.id}`;
  let flow = flows.get(key);
  if (!flow) {
    flow = createReviewSession({ id: message.id, paths: (message.data?.paths as string[]) ?? [], includeAll: Boolean(message.data?.includeAll),
      saved: message.data?.progress as ReviewSnapshot | undefined,
      persist: progress => api.saveReview(sessionId, message.id, progress as unknown as Record<string, unknown>),
    });
    flows.set(key, flow);
  }
  return flow;
}
/** 换库前释放会话缓存，防止前一知识库内容留在下一工作区。 */
export function clearAgentReviews(): void { flows.clear(); }
/** 评分请求完成前不能换库或启动会话写入，避免迟到结果修改另一知识库。 */
export function agentReviewsBusy(): boolean { return [...flows.values()].some(flow => flow.busy.value || flow.loading.value); }
/** 界面退出先卸载交互，再等待已经提交的评分落盘。 */
export async function settleAgentReviews(): Promise<void> { while (agentReviewsBusy()) await new Promise(resolve => setTimeout(resolve, 100)); }
