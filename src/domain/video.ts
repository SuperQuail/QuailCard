/** 视频转笔记的 wire 类型，与后端 src-tauri/src/video/models.rs 同步。 */

/** 分 P 摘要。 */
export interface VideoPage {
  page: number;
  title: string;
  duration: number;
}

/** 清晰度选项。 */
export interface VideoQuality {
  qn: number;
  label: string;
  height: number;
  available: boolean;
  requiresVip: boolean;
  estimatedBytes: number;
  estimatedDuration?: number;
  unavailableReason?: string;
}

/** 链接解析结果。 */
export interface VideoProbe {
  bvid: string;
  title: string;
  owner: string;
  duration: number;
  pages: VideoPage[];
  qualities: VideoQuality[];
  selectedPage?: number;
  loggedIn: boolean;
}

/** 任务状态快照。 */
export interface VideoTaskStatus {
  taskId: string;
  state: "running" | "completed" | "failed" | "cancelled" | string;
  step: string;
  progress: number;
  message: string;
  sequence: number;
  segments: number;
  shots: number;
  transcriptSource: string;
  backend?: string;
  asrProgress?: AsrProgress | null;
  logs?: string[];
  notePath: string | null;
  error: string | null;
}

/** 当前分 P/尝试的 Whisper 实测回调；总时长为元数据，不是精确识别时间戳。 */
export interface AsrProgress {
  page: number;
  attempt: number;
  percent: number | null;
  totalAudioSeconds: number | null;
  elapsedSeconds: number;
}

/** 扫码登录状态。 */
export interface VideoLoginStatus {
  state: "idle" | "starting" | "waiting" | "scanned" | "confirmed" | "expired" | "timeout" | "failed" | string;
  message: string;
  image: string;
  loggedIn: boolean;
  /** 已登录时的 B 站昵称；未登录或接口未返回时为空。 */
  name?: string;
  /** 已登录时的头像地址；界面只使用后端代理结果，不直接加载该地址。 */
  avatar?: string;
}

/** 外部组件状态。 */
export interface VideoComponentStatus {
  /** 稳定标识（ffmpeg / whisper）；界面按它把设置项关联到实际解析结果。 */
  id: string;
  name: string;
  available: boolean;
  path: string;
  source: string;
  detail?: string;
}

/** 语音模型状态。 */
export interface VideoModelStatus {
  id: string;
  label: string;
  bytes: number;
  status: "missing" | "invalid" | "ready" | string;
}

/** 视频设置。 */
export interface VideoSettings {
  noteFolder: string;
  asrModel: string;
  asrEnabled: boolean;
  screenshotsEnabled: boolean;
  maxShots: number;
  videoQuality: string;
  preferCodec: string;
  videoMaxDownloadMb: number;
  shotMaxWidth: number;
  ffmpegPath: string;
  whisperPath: string;
  modelMirror: string;
  keepMediaDays: number;
}

/** 输出模式：note 生成结构化笔记，transcript 输出保留时间轴的字幕稿。 */
export type VideoMode = "note" | "transcript";

/** 开始任务的输入。 */
export interface VideoStartInput {
  url: string;
  providerId: string;
  quality: number | null;
  pages: number[];
  screenshots: boolean | null;
  forceTranscribe?: boolean;
  /** 输出模式；wire 字段为 camelCase，未传等价于 note（后端 serde default）。 */
  mode: VideoMode;
}

/** 转录来源的中文展示名。 */
export function transcriptLabel(source: string): string {
  if (source === "bilibili_ai") return "B 站 AI 字幕";
  if (source === "bilibili_cc") return "B 站字幕";
  if (source === "whisper") return "本地转写";
  return source || "未知来源";
}

/** 秒数格式化为 MM:SS 或 HH:MM:SS。 */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds || 0));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const pad = (value: number): string => String(value).padStart(2, "0");
  return hours > 0 ? pad(hours) + ":" + pad(minutes) + ":" + pad(secs) : pad(minutes) + ":" + pad(secs);
}

/** 字节数格式化，用于清晰度与模型体积提示。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  if (bytes < 1024) return Math.round(bytes) + "B";
  if (bytes < 1024 * 1024) return Math.round(bytes / 1024) + "KB";
  const megabytes = bytes / 1024 / 1024;
  if (megabytes >= 1024) return (megabytes / 1024).toFixed(1) + "GB";
  return Math.round(megabytes) + "MB";
}

/** 模型下载当前进度，终态继续保留供界面读取。 */
export interface VideoDownloadStatus {
  modelId: string;
  state: "idle" | "downloading" | "ready" | "failed" | "cancelled" | string;
  downloadedBytes: number;
  totalBytes: number | null;
  bytesPerSecond: number;
  error: string | null;
}

/** 历史任务仅恢复参数，使用当前供应商与设置再次执行。 */
export interface VideoTaskHistory {
  taskId: string;
  url: string;
  title: string;
  state: string;
  updatedAt: number;
  notePath: string | null;
  error: string | null;
  pages: number[];
  quality: number | null;
}
