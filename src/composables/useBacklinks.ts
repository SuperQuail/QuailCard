import { ref, watch, type Ref } from "vue";
import * as backend from "../api/backend";
import type { Backlink } from "../domain/types";
import { savedAt } from "../services/stores/noteStore";

/** 保存后多久再查反链：等落盘与索引刷新完成，避免读到上一版正文。 */
const RELOAD_DELAY_MS = 400;

/**
 * 反链查询：跟随当前笔记路径与保存时机自动刷新。
 * 反链由后端按索引计算，前端只负责节流与丢弃过期响应。
 */
export function useBacklinks(notePath: Ref<string | null>) {
  const backlinks = ref<Backlink[]>([]);
  const loading = ref(false);
  /** 请求代号：切走笔记后迟到的响应不能覆盖新笔记的结果。 */
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  /** 真正发起查询；无笔记时直接清空，不发请求。 */
  async function load(): Promise<void> {
    const path = notePath.value;
    const current = ++generation;
    if (!path) {
      backlinks.value = [];
      loading.value = false;
      return;
    }
    loading.value = true;
    try {
      const found = await backend.getBacklinks(path);
      if (current === generation) backlinks.value = found;
    } catch {
      // 反链只是辅助视图，查询失败不打断编辑：清空并静默，避免弹窗干扰。
      if (current === generation) backlinks.value = [];
    } finally {
      if (current === generation) loading.value = false;
    }
  }

  /** 合并连续的保存事件，避免每敲一个字都查一次。 */
  function schedule(): void {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void load();
    }, RELOAD_DELAY_MS);
  }

  watch([notePath, savedAt], schedule, { immediate: true });

  return { backlinks, loading };
}
