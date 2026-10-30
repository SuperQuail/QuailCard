import { computed, type Ref } from "vue";
import { notes, activeNotePath } from "../services/stores/noteStore";
import { sidePanelTab } from "../services/stores/uiStore";
import { resolveLink } from "../domain/links";
import { splitWikiTarget } from "../markdown/wikilink";
import { useBacklinks } from "./useBacklinks";
import type { ConfirmRequest } from "./useConfirm";
import type { NoteSummary } from "../domain/types";

/** 编辑器需要暴露的定位能力；用接口而非组件类型，避免组合式函数反向依赖组件。 */
export interface EditorReveal {
  /** 滚动到正文内的字符偏移。 */
  revealPosition(pos: number): void;
  /** 滚动到 1 起始的行号。 */
  revealLine(line: number): void;
}

/** 侧栏与双链跳转的依赖，全部由壳层注入。 */
export interface NoteSidebarsOptions {
  /** 编辑器实例：大纲与反链跳转的落点。 */
  editor: Ref<EditorReveal | null>;
  /** 打开指定笔记（含窄屏收抽屉等壳层副作用）。 */
  openNote: (path: string) => Promise<void>;
  /** 未解析链接时新建笔记；返回是否创建成功。 */
  createNote: (title: string, folder: string) => Promise<boolean>;
  /** 通用确认框：新建笔记前确认，避免误点凭空生成文件。 */
  askConfirm: (request: ConfirmRequest) => Promise<boolean>;
}

/**
 * 左栏页签、大纲跳转、反链与 [[双链]] 打开的编排。
 *
 * 为什么单独成文件：App.vue 已接近 500 行上限，而这几件事只依赖注入的回调，
 * 抽出来既不引入新的状态来源，也让壳层组件只做装配。
 */
export function useNoteSidebars(options: NoteSidebarsOptions) {
  const { backlinks } = useBacklinks(computed(() => activeNotePath.value));

  /** 点击大纲标题：交给编辑器滚动到对应偏移。 */
  function jumpToHeading(from: number): void {
    options.editor.value?.revealPosition(from);
  }

  /** 点开反链：先切到来源笔记，再滚到链接所在行。 */
  async function openBacklink(path: string, line: number): Promise<void> {
    await options.openNote(path);
    options.editor.value?.revealLine(line);
  }

  /** 双链的所在目录：未解析链接在当前笔记同级创建，符合就近归档的直觉。 */
  function folderOf(path: string): string {
    const slash = path.lastIndexOf("/");
    return slash < 0 ? "" : path.slice(0, slash);
  }

  /**
   * 打开 [[双链]]：已解析直接跳转；未解析先确认再新建。
   * 不做"静默新建"，是因为误点链接在笔记里凭空生成空文件比什么都不做更糟。
   */
  async function openLink(target: string): Promise<void> {
    const from = activeNotePath.value;
    // #标题 只影响跳转位置，不参与"这是哪篇笔记"的判定。
    const wanted = splitWikiTarget(target).note.trim();
    if (!from || !wanted) return;
    const resolved = resolveLink(wanted, from, notes.value as NoteSummary[]);
    if (resolved) {
      await options.openNote(resolved);
      return;
    }
    const confirmed = await options.askConfirm({
      title: "新建笔记",
      body: `「${wanted}」还不存在，要在当前文件夹新建它吗？`,
      confirmLabel: "新建",
      danger: false,
    });
    if (confirmed) {
      await options.createNote(wanted, folderOf(from));
    }
  }

  return { backlinks, jumpToHeading, openBacklink, openLink, sidePanelTab };
}
