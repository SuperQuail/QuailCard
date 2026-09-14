import { mount } from "@vue/test-utils";
import { expect, test } from "vitest";
import StatusBar from "./StatusBar.vue";

/** 常驻底栏保留原有元信息，不增加顶部移除的保存状态。 */
test("底栏保留路径、字数、复习和主题，不显示保存提示", async () => {
  const wrapper = mount(StatusBar, { props: { vaultName: "学习库", noteTitle: "02 线性表", wordCount: 1241, dueCount: 111, dark: false } });
  try {
    expect(wrapper.get("footer").classes()).toContain("shrink-0");
    expect(wrapper.text()).toContain("学习库");
    expect(wrapper.text()).toContain("02 线性表");
    expect(wrapper.text()).toContain("1241 字");
    expect(wrapper.text()).toContain("今日待复习 111");
    expect(wrapper.text()).not.toContain("已保存");
    expect(wrapper.find('[role="status"]').exists()).toBe(false);
    await wrapper.get('button[title="切换主题"]').trigger("click");
    expect(wrapper.emitted("toggle-theme")).toEqual([[]]);
    await wrapper.setProps({ noteTitle: "", wordCount: 0, dark: true });
    expect(wrapper.find("footer").exists()).toBe(true);
    expect(wrapper.text()).toContain("深色");
  } finally { wrapper.unmount(); }
});
