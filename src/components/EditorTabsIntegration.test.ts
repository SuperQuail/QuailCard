import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, ref } from "vue";
import { expect, test, vi } from "vitest";
import type { NoteSummary } from "../domain/types";
import type { NotePathChange } from "../domain/notePaths";
import { useNoteTabs } from "../composables/useNoteTabs";
import EditorPane from "./EditorPane.vue";

/** 用真实标签状态与编辑器连接 props/emits，读写端口仅用内存替身。 */
function workspace() {
  const flush = vi.fn(async (_path: string) => {});
  const error = vi.fn();
  const host = defineComponent({
    components: { EditorPane },
    setup() {
      const notes = ref<NoteSummary[]>(["a", "b"].map((name) => ({ path: name + ".md", title: name, tagsJson: "[]", mtime: 1, cardCount: 0, dueCount: 0 })));
      const activePath = ref<string | null>("a.md");
      const content = ref("# a\n\n正文 a");
      const documents = new Map([["a.md", content.value], ["b.md", "# b\n\n正文 b"]]);
      /** 模拟既有成功选中文档的发布顺序。 */
      async function select(path: string): Promise<void> { activePath.value = path; content.value = documents.get(path)!; }
      /** 关闭最后一页只清当前选择，文件列表保持不变。 */
      function clearActive(): void { activePath.value = null; content.value = ""; }
      /** 模拟保存服务即时接收草稿，供重新激活时读取。 */
      function save(path: string, text: string): void { documents.set(path, text); if (activePath.value === path) content.value = text; }
      const { tabs, close, closing } = useNoteTabs({ notes, activePath, vaultPath: ref("vault"), pathChange: ref<NotePathChange | null>(null), busy: ref(false), select, flush, clearActive, onError: error });
      return { notes, activePath, content, tabs, close, closing, select, save };
    },
    template: '<EditorPane v-if="activePath" :note-path="activePath" :content="content" :dark="false" :panel-open="false" :tabs="tabs" :tabs-busy="closing" @select-tab="select" @close-tab="close" @save-content="save" /><p v-else>空工作区</p>',
  });
  return { wrapper: mount(host), flush, error };
}

/** 验证真实标签点击通路和最后一页空状态，避免只有孤立组件测试通过。 */
test("标签切换、关闭邻页、关闭全部贯通到真实编辑器", async () => {
  const { wrapper, flush } = workspace();
  try {
    await flushPromises();
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(1);
    await wrapper.vm.select("b.md");
    await flushPromises();
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(2);
    expect(wrapper.get('[role="tabpanel"]').attributes("aria-labelledby")).toBe("note-tab-b.md");
    expect(wrapper.findAll("header")).toHaveLength(1);
    expect(wrapper.get(".tab-actions header").classes()).not.toContain("absolute");
    await wrapper.get('[role="tab"][aria-label="a.md"]').trigger("click");
    await flushPromises();
    expect(wrapper.get(".cm-editor").text()).toContain("正文 a");
    await wrapper.get('[aria-label="关闭标签：a.md"]').trigger("click");
    await flushPromises();
    expect(flush).toHaveBeenCalledWith("a.md");
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(1);
    expect(wrapper.get(".cm-editor").text()).toContain("正文 b");
    await wrapper.get('[aria-label="关闭标签：b.md"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toBe("空工作区");
    expect(wrapper.vm.notes).toHaveLength(2);
  } finally { wrapper.unmount(); }
});

/** 保存失败时编辑器和标签都必须保留，不能只保留后台草稿而关掉页面。 */
test("保存失败不关闭当前编辑器", async () => {
  const { wrapper, flush, error } = workspace();
  try {
    await flushPromises();
    flush.mockRejectedValueOnce(new Error("写入失败"));
    await wrapper.get('[aria-label="关闭标签：a.md"]').trigger("click");
    await flushPromises();
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(1);
    expect(wrapper.get(".cm-editor").text()).toContain("正文 a");
    expect(error).toHaveBeenCalledTimes(1);
  } finally { wrapper.unmount(); }
});
