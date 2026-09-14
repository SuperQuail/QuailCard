import * as api from "../../api/video";
import type { VideoComponentStatus, VideoDownloadStatus, VideoModelStatus, VideoSettings } from "../../domain/video";
import { resolveError } from "../../utils/errorMessage";

interface ResourceState {
  settings: VideoSettings | null;
  components: VideoComponentStatus[];
  models: VideoModelStatus[];
  downloadingModel: string;
  downloadStatus: VideoDownloadStatus | null;
  error: string;
}

/** 设置与模型命令独立编排，防止并发整对象保存丢失字段。 */
export function createVideoResources(state: ResourceState) {
  let queue: Promise<void> = Promise.resolve();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let settingsRevision = 0;
  let owned = false;

  /** 完成在途设置写入后再读取，避免工作区重开覆盖新设置。 */
  async function load(): Promise<void> {
    await queue;
    const revision = settingsRevision;
    const settings = await api.getSettings();
    await queue;
    if (revision === settingsRevision) state.settings = settings;
    await refresh();
  }

  /** 设置补丁在执行时合并最新回读值，而不是点击时捕获旧整对象。 */
  function save(patch: Partial<VideoSettings>): Promise<void> {
    const captured = { ...patch };
    settingsRevision += 1;
    queue = queue.then(async () => {
      if (!state.settings) return;
      try { state.settings = await api.saveSettings({ ...state.settings, ...captured }); }
      catch (error) { state.error = resolveError(error); }
    });
    return queue;
  }

  /** 组件与模型检测失败保留上一份可信快照。 */
  async function refreshModels(): Promise<void> {
    try {
      const [components, models] = await Promise.all([api.components(), api.models()]);
      state.components = components;
      state.models = models;
    } catch (error) { state.error = resolveError(error); }
  }

  /** 重开页面接管后端已有下载，不要求原来的阻塞命令仍存在。 */
  async function refresh(): Promise<void> {
    await refreshModels();
    if (state.downloadingModel) return;
    const epoch = generation;
    try {
      const status = await api.downloadStatus();
      if (epoch !== generation || state.downloadingModel) return;
      state.downloadStatus = status;
      if (status.state === "downloading" && status.modelId) {
        state.downloadingModel = status.modelId;
        const next = ++generation;
        timer = setTimeout(() => { void poll(status.modelId, next); }, 1000);
      }
    } catch { /* 状态服务临时不可用时仍可使用已检测的组件。 */ }
  }

  /** 下载查询完成后再计时；接管的任务只依据后端终态释放锁。 */
  async function poll(id: string, epoch: number): Promise<void> {
    try {
      const status = await api.downloadStatus();
      if (epoch !== generation) return;
      if (status.modelId === id || !owned) state.downloadStatus = status;
      if (!owned && status.state === "downloading" && status.modelId && status.modelId !== id) {
        state.downloadingModel = status.modelId;
        const next = ++generation;
        timer = setTimeout(() => { void poll(status.modelId, next); }, 1000);
        return;
      }
      if (!owned && status.state !== "downloading") {
        generation += 1;
        state.downloadingModel = "";
        if (status.error) state.error = status.error;
        await refreshModels();
        return;
      }
    } catch { /* 进度查询失败不宣告下载失败，下一周期继续尝试。 */ }
    if (epoch === generation && state.downloadingModel === id) {
      timer = setTimeout(() => { void poll(id, epoch); }, 1000);
    }
  }

  /** 本轮下载持锁直到命令返回，失败与取消也重新检测真实安装状态。 */
  async function download(id: string): Promise<void> {
    if (state.downloadingModel) return;
    const epoch = ++generation;
    owned = true;
    state.downloadingModel = id;
    state.downloadStatus = null;
    state.error = "";
    try {
      const operation = api.downloadModel(id);
      void poll(id, epoch);
      await operation;
    } catch (error) { state.error = resolveError(error); }
    finally {
      generation += 1;
      if (timer !== undefined) clearTimeout(timer);
      // 旧进度请求已失效，最终回读不会被较慢的旧快照覆盖。
      try {
        const status = await api.downloadStatus();
        if (status.modelId === id) state.downloadStatus = status;
      } catch { state.downloadStatus = null; }
      await refreshModels();
      owned = false;
      state.downloadingModel = "";
    }
  }

  /** 取消只提交取消信号，等待下载命令或接管轮询落定后释放锁。 */
  async function cancel(): Promise<void> {
    if (!state.downloadingModel) return;
    try { await api.cancelModel(); }
    catch (error) { state.error = resolveError(error); }
  }

  return { load, save, refresh, download, cancel };
}
