import type {
  VideoComponentStatus,
  VideoLoginStatus,
  VideoModelStatus,
  VideoProbe,
  VideoSettings,
  VideoStartInput,
  VideoTaskStatus,
} from "../../domain/video";

/**
 * 浏览器演示后端的视频命令：只提供形状正确的假数据。
 *
 * 真正的解析、转写与截图全部在桌面端后端完成，这里不复制任何业务规则。
 */
const settings: VideoSettings = {
  noteFolder: "视频笔记",
  asrModel: "small",
  asrEnabled: true,
  screenshotsEnabled: true,
  maxShots: 12,
  videoQuality: "auto",
  preferCodec: "avc",
  videoMaxDownloadMb: 300,
  shotMaxWidth: 1600,
  ffmpegPath: "",
  whisperPath: "",
  modelMirror: "",
  keepMediaDays: 7,
};

let task: VideoTaskStatus | null = null;

/** 演示用解析结果。 */
const probe: VideoProbe = {
  bvid: "BV1Qwby6DEu1",
  title: "演示视频：浏览器模式不会访问 B 站",
  owner: "演示 UP 主",
  duration: 754,
  pages: [
    { page: 1, title: "开场", duration: 300 },
    { page: 2, title: "正文", duration: 454 },
  ],
  qualities: [
    { qn: 32, label: "480P 清晰", height: 480, available: true, requiresVip: false, estimatedBytes: 12_000_000 },
    { qn: 64, label: "720P 高清", height: 720, available: false, requiresVip: false, estimatedBytes: 24_000_000 },
    { qn: 80, label: "1080P 高清", height: 1080, available: false, requiresVip: true, estimatedBytes: 48_000_000 },
  ],
  loggedIn: false,
};

/** 命令分发；未知命令明确报错，避免静默成功。 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  switch (command) {
    case "video_download_status":
      return { modelId: "", state: "idle", downloadedBytes: 0, totalBytes: null, bytesPerSecond: 0, error: null } as T;
    case "video_history":
      return [] as T;
    case "video_get_settings":
      return { ...settings } as T;
    case "video_save_settings":
      Object.assign(settings, (args?.settings ?? {}) as Partial<VideoSettings>);
      return { ...settings } as T;
    case "video_components": {
      const components: VideoComponentStatus[] = [
        { id: "ffmpeg", name: "ffmpeg（媒体解码与截图）", available: false, path: "", source: "演示环境" },
        { id: "whisper", name: "whisper-cli（本地转写）", available: false, path: "", source: "演示环境" },
      ];
      return components as T;
    }
    case "video_models": {
      const models: VideoModelStatus[] = [
        { id: "base", label: "base（142MB，最快）", bytes: 147_951_465, status: "missing" },
        { id: "small", label: "small（466MB，推荐）", bytes: 487_601_967, status: "missing" },
      ];
      return models as T;
    }
    case "video_probe":
      return structuredClone(probe) as T;
    case "video_login_start": {
      const status: VideoLoginStatus = { state: "failed", message: "浏览器演示环境不支持扫码登录", image: "", loggedIn: false };
      return status as T;
    }
    case "video_login_status":
      return { state: "idle", message: "未开始扫码", image: "", loggedIn: false } as T;
    case "video_avatar":
      return "" as T;
    case "video_login_cancel":
    case "video_logout":
    case "video_model_cancel":
      return undefined as T;
    case "video_task_start": {
      // 演示只回显 wire 输入中的输出模式，不复制后端的模式规则（缺省与 serde default 一致为 note）。
      const input = (args?.input ?? {}) as Partial<VideoStartInput>;
      task = {
        taskId: "demo-video-task",
        state: "running",
        step: "演示中",
        progress: 40,
        message: `浏览器演示不会真正下载视频（输出模式：${input.mode ?? "note"}）`,
        sequence: 1,
        segments: 0,
        shots: 0,
        transcriptSource: "",
        notePath: null,
        error: null,
      };
      return structuredClone(task) as T;
    }
    case "video_task_status":
      return structuredClone(task ?? { taskId: "", state: "cancelled", step: "", progress: 0, message: "", sequence: 0, segments: 0, shots: 0, transcriptSource: "", notePath: null, error: null }) as T;
    case "video_task_cancel":
      if (task) task = { ...task, state: "cancelled", step: "已停止" };
      return undefined as T;
    case "video_download_model": {
      const id = String(args?.id ?? "small");
      const models: VideoModelStatus[] = [{ id, label: id, bytes: 0, status: "missing" }];
      return models[0] as T;
    }
    default:
      throw new Error("浏览器演示环境不支持该命令：" + command);
  }
}
