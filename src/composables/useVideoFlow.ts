import {
  closeLogin,
  cancelModel,
  retryTask,
  refreshHistory,
  restoreHistory,
  closeVideoWorkspace,
  downloadModel,
  logout,
  openLogin,
  openVideoWorkspace,
  probeLink,
  refreshComponents,
  saveSettings,
  startTask,
  stopTask,
  togglePage,
  toggleSettings,
  videoState,
} from "../services/stores/videoStore";

/** 工作区只把受控事件交给用例，组件不接触 api 与全量状态。 */
export function useVideoFlow() {
  return {
    state: videoState,
    open: openVideoWorkspace,
    close: closeVideoWorkspace,
    probe: probeLink,
    togglePage,
    start: startTask,
    stop: stopTask,
    retry: retryTask,
    refreshHistory,
    restoreHistory,
    cancelModel,
    login: openLogin,
    closeLogin,
    logout,
    downloadModel,
    saveSettings,
    refreshComponents,
    toggleSettings,
  };
}
