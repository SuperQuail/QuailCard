import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./backend";
import type {
  VideoComponentStatus,
  VideoDownloadStatus,
  VideoTaskHistory,
  VideoLoginStatus,
  VideoModelStatus,
  VideoProbe,
  VideoSettings,
  VideoStartInput,
  VideoTaskStatus,
} from "../domain/video";

/** 浏览器只使用演示数据，真实请求始终走后端。 */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return invoke<T>(command, args);
  return (await import("../dev/mockBackend/video")).call<T>(command, args);
}

/** 解析链接并返回视频信息与清晰度选项。 */
export function probe(url: string): Promise<VideoProbe> {
  return call("video_probe", { url });
}

/** 开始扫码登录。 */
export function loginStart(): Promise<VideoLoginStatus> {
  return call("video_login_start");
}

/** 查询扫码状态；确认后后端会写入保险库。 */
export function loginStatus(): Promise<VideoLoginStatus> {
  return call("video_login_status");
}

/** 取消扫码会话。 */
export function loginCancel(): Promise<void> {
  return call("video_login_cancel");
}

/** 退出 B 站登录。 */
export function logout(): Promise<void> {
  return call("video_logout");
}

/** 读取已登录用户头像（后端代理为 data URL）；未登录返回空串。 */
export function avatar(): Promise<string> {
  return call("video_avatar");
}

/** 开始视频任务。 */
export function start(input: VideoStartInput): Promise<VideoTaskStatus> {
  return call("video_task_start", { input });
}

/** 查询任务快照。 */
export function status(id: string): Promise<VideoTaskStatus> {
  return call("video_task_status", { id });
}

/** 停止任务。 */
export function cancel(id: string): Promise<void> {
  return call("video_task_cancel", { id });
}

/** 外部组件状态。 */
export function components(): Promise<VideoComponentStatus[]> {
  return call("video_components");
}

/** 语音模型清单。 */
export function models(): Promise<VideoModelStatus[]> {
  return call("video_models");
}

/** 下载语音模型。 */
export function downloadModel(id: string): Promise<VideoModelStatus> {
  return call("video_download_model", { id });
}

/** 取消模型下载。 */
export function cancelModel(): Promise<void> {
  return call("video_model_cancel");
}

/** 读取视频设置。 */
export function getSettings(): Promise<VideoSettings> {
  return call("video_get_settings");
}

/** 保存视频设置。 */
export function saveSettings(settings: VideoSettings): Promise<VideoSettings> {
  return call("video_save_settings", { settings });
}

/** 读取当前窗口模型下载状态，不触发下载。 */
export function downloadStatus(): Promise<VideoDownloadStatus> {
  return call("video_download_status");
}

/** 查询当前知识库历史任务，不自动启动或恢复执行。 */
export function history(): Promise<VideoTaskHistory[]> {
  return call("video_history");
}
