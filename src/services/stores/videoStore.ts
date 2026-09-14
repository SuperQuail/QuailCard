import { reactive, watch } from "vue";
import * as api from "../../api/video";
import * as backend from "../../api/backend";
import * as noteStore from "./noteStore";
import { vaultPath } from "./vaultStore";
import type { VideoComponentStatus, VideoDownloadStatus, VideoLoginStatus, VideoModelStatus,
  VideoMode, VideoProbe, VideoSettings, VideoTaskHistory, VideoTaskStatus } from "../../domain/video";
import { resolveError } from "../../utils/errorMessage";
import { looksLikeBilibili } from "../videoLink";
import { activeProviderId } from "./providerStore";
import { createVideoLogin } from "./videoLogin";
import { chooseQuality, inputPage } from "./videoSelection";
import { createVideoTasks } from "./videoTasks";
import { createVideoResources } from "./videoResources";

/** 视频工作区状态；异步控制器只接收各自所需状态。 */
export const videoState = reactive({
  open: false, url: "", probe: null as VideoProbe | null, probeUrl: "",
  quality: 0, pages: [] as number[], screenshots: true, forceTranscribe: false, mode: "note" as VideoMode,
  probing: false, starting: false, error: "", taskError: "",
  task: null as VideoTaskStatus | null, login: null as VideoLoginStatus | null,
  loginOpen: false, loading: false, components: [] as VideoComponentStatus[],
  models: [] as VideoModelStatus[], settings: null as VideoSettings | null,
  settingsOpen: false, downloadingModel: "", downloadStatus: null as VideoDownloadStatus | null,
  history: [] as VideoTaskHistory[], historyError: "",
  avatar: "",
});
let probeGeneration = 0;
const resources = createVideoResources(videoState);
/** 生成只补索引和侧栏，不选择笔记或覆盖编辑器草稿；跨 Vault 完成直接忽略。 */
const tasks = createVideoTasks(videoState, () => {
  const originVault = vaultPath.value;
  return async (notePath) => {
    if (!originVault || vaultPath.value !== originVault) return;
    await backend.syncNoteIndex(notePath);
    if (vaultPath.value !== originVault) return;
    await noteStore.refreshNotes();
  };
});

/** 无论来自输入框还是粘贴入口，URL 改变同步撤销旧解析结果写权限。 */
watch(() => videoState.url, () => {
  probeGeneration += 1;
  videoState.probe = null;
  videoState.probeUrl = "";
  videoState.probing = false;
}, { flush: "sync" });

/** 打开时恢复登录与历史记录，不中断正在执行的任务。 */
export async function openVideoWorkspace(): Promise<void> {
  videoState.open = true;
  if (videoState.loading) return;
  videoState.loading = true;
  videoState.error = "";
  try {
    await Promise.all([resources.load(), loginController.checkStatus(), refreshHistory()]);
    // 头像只在首次进入时补取；退出登录由登录控制器清空。
    if (videoState.login?.loggedIn && !videoState.avatar) await loadAvatar();
  }
  catch (error) { videoState.error = resolveError(error); }
  finally { videoState.loading = false; }
}

/** 关闭仅影响展示，后台任务继续收敛到终态。 */
export function closeVideoWorkspace(): void { videoState.open = false; }

/** 明确绑定 URL 和请求代次；登录刷新只调整失效选择，不重置用户偏好。 */
export async function probeLink(preserve = false): Promise<void> {
  const url = videoState.url.trim();
  const epoch = ++probeGeneration;
  const previous = preserve && videoState.probeUrl === url ? videoState.probe : null;
  if (!looksLikeBilibili(url)) {
    videoState.probe = null;
    videoState.probeUrl = "";
    videoState.probing = false;
    videoState.error = "暂时只支持 B 站视频链接";
    return;
  }
  videoState.probing = true;
  videoState.error = "";
  if (!previous) { videoState.probe = null; videoState.probeUrl = ""; }
  try {
    const probe = await api.probe(url);
    if (epoch !== probeGeneration || videoState.url.trim() !== url) return;
    videoState.probe = probe;
    videoState.probeUrl = url;
    if (!previous || !probe.qualities.some(item => item.qn === videoState.quality && item.available)) {
      videoState.quality = chooseQuality(probe, videoState.settings?.videoQuality);
    }
    const retained = previous ? videoState.pages.filter(page => probe.pages.some(item => item.page === page)) : [];
    const selected = probe.selectedPage ?? inputPage(url);
    const first = probe.pages.find(page => page.page === selected) ?? probe.pages[0];
    videoState.pages = retained.length ? retained : first ? [first.page] : [];
    // mode 是用户偏好：首次解析与重新解析都不重置，避免换链接后丢失输出模式。
    if (!previous) videoState.screenshots = videoState.settings?.screenshotsEnabled ?? true;
  } catch (error) {
    if (epoch === probeGeneration) {
      videoState.error = resolveError(error);
      videoState.probe = null;
      videoState.probeUrl = "";
    }
  } finally { if (epoch === probeGeneration) videoState.probing = false; }
}

/** 分 P 必须来自当前解析结果，且至少保留一个。 */
export function togglePage(page: number): void {
  if (!videoState.probe?.pages.some(item => item.page === page)) return;
  const index = videoState.pages.indexOf(page);
  if (index >= 0) { if (videoState.pages.length > 1) videoState.pages.splice(index, 1); }
  else { videoState.pages.push(page); videoState.pages.sort((a, b) => a - b); }
}

/** 任务输入只能来自与当前 URL 绑定且仍可用的解析结果。 */
export async function startTask(): Promise<void> {
  if (videoState.starting || videoState.task?.state === "running") return;
  if (videoState.probing || !videoState.probe || videoState.probeUrl !== videoState.url.trim()) {
    videoState.error = "请先解析当前链接"; return;
  }
  if (!videoState.pages.length || !videoState.pages.every(page => videoState.probe?.pages.some(item => item.page === page))
    || !videoState.probe.qualities.some(item => item.qn === videoState.quality && item.available)) {
    videoState.error = "请选择可用清晰度与分 P"; return;
  }
  if (!activeProviderId.value) { videoState.error = "请先在设置中选择模型供应商"; return; }
  await tasks.start({ url: videoState.probeUrl, providerId: activeProviderId.value,
    quality: videoState.quality, pages: [...videoState.pages], screenshots: videoState.screenshots,
    forceTranscribe: videoState.forceTranscribe, mode: videoState.mode });
}

/** 头像由后端代理成 data URL；读取失败只影响展示，不改变登录状态。 */
export async function loadAvatar(): Promise<void> {
  if (!videoState.login?.loggedIn) { videoState.avatar = ""; return; }
  try { videoState.avatar = (await api.avatar()) || ""; }
  catch { videoState.avatar = ""; }
}

/** 登录变化后重新验证可用档位，同时保留仍有效的选择。 */
async function refreshAfterLogin(): Promise<void> {
  const probing = videoState.probe || videoState.probing ? probeLink(true) : Promise.resolve();
  await Promise.all([probing, loadAvatar()]);
}
const loginController = createVideoLogin(videoState, refreshAfterLogin);

/** 恢复历史只填入参数，必须确认后才使用当前供应商与设置执行。 */
export async function restoreHistory(item: VideoTaskHistory): Promise<void> {
  videoState.url = item.url;
  const restoring = probeLink();
  const epoch = probeGeneration;
  await restoring;
  if (epoch !== probeGeneration || !videoState.probe || videoState.probeUrl !== item.url.trim()) return;
  const pages = item.pages.filter(page => videoState.probe?.pages.some(candidate => candidate.page === page));
  if (pages.length) videoState.pages = pages;
  if (videoState.probe.qualities.some(quality => quality.qn === item.quality && quality.available)) videoState.quality = item.quality ?? 0;
  videoState.forceTranscribe = false;
  // 历史记录不保存 mode：恢复参数保持用户当前输出模式，避免意外把字幕模式切回笔记。
}

/** 历史加载失败不影响当前任务；已有记录保持可读。 */
export async function refreshHistory(): Promise<void> {
  try { videoState.history = await api.history(); videoState.historyError = ""; }
  catch (error) { videoState.historyError = resolveError(error); }
}

/** 新终态触发历史回读，不创建额外任务轮询。 */
watch(() => videoState.task?.state, state => { if (state && state !== "running") void refreshHistory(); });

/** 以下入口保持组件事件简单，并将并发策略留在专用控制器。 */
export const stopTask = tasks.stop;
export const retryTask = tasks.retry;
export const openLogin = loginController.open;
export const closeLogin = loginController.close;
export const logout = loginController.logout;
export const downloadModel = resources.download;
export const cancelModel = resources.cancel;
export const refreshComponents = resources.refresh;
export const saveSettings = resources.save;

/** 展开或收起原有设置面板。 */
export function toggleSettings(): void { videoState.settingsOpen = !videoState.settingsOpen; }

/** 登录提示与表单统一只读取控制器状态。 */
export function needsLogin(): boolean { return !videoState.login?.loggedIn; }
