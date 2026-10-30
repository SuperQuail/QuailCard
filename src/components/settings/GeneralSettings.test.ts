import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import GeneralSettings from "./GeneralSettings.vue";

/** 通用设置的必需 props：测试只关心文件删除开关。 */
const baseProps = {
  vaultPath: "D:/Vault",
  recentVaults: [],
  aiGradingEnabled: false,
  permanentDelete: false,
  updateLinksOnRename: true,
  attachmentFolder: "attachments",
  attachmentFolderSaving: false,
  attachmentFolderStatus: "",
  dataLocations: { cardsDir: null, configDir: "C:/Users/demo/AppData/Roaming/QuailCard" },
};

describe("通用设置：文件小节", () => {
  test("展示永久删除开关与不可恢复说明", () => {
    const wrapper = mount(GeneralSettings, { props: baseProps });
    expect(wrapper.text()).toContain("文件");
    expect(wrapper.text()).toContain("永久删除（不经过回收站）");
    expect(wrapper.text()).toContain("开启后删除无法恢复");
    const toggle = wrapper.get('[aria-label="开启永久删除"]');
    expect(toggle.attributes("role")).toBe("switch");
    expect(toggle.attributes("aria-checked")).toBe("false");
    wrapper.unmount();
  });

  test("点击开关上抛开启事件", async () => {
    const wrapper = mount(GeneralSettings, { props: baseProps });
    await wrapper.get('[aria-label="开启永久删除"]').trigger("click");
    expect(wrapper.emitted("update-permanent-delete")).toEqual([[true]]);
    wrapper.unmount();
  });

  test("已开启时开关高亮，点击上抛关闭事件", async () => {
    const wrapper = mount(GeneralSettings, { props: { ...baseProps, permanentDelete: true } });
    const toggle = wrapper.get('[aria-label="关闭永久删除"]');
    expect(toggle.attributes("aria-checked")).toBe("true");
    expect(toggle.classes()).toContain("is-on");
    await toggle.trigger("click");
    expect(wrapper.emitted("update-permanent-delete")).toEqual([[false]]);
    wrapper.unmount();
  });

  test("改名联动开关默认开启，点击上抛关闭事件", async () => {
    const wrapper = mount(GeneralSettings, { props: baseProps });
    expect(wrapper.text()).toContain("改名时更新引用链接");
    const toggle = wrapper.get('[aria-label="关闭改名时更新引用链接"]');
    expect(toggle.attributes("role")).toBe("switch");
    expect(toggle.attributes("aria-checked")).toBe("true");
    await toggle.trigger("click");
    expect(wrapper.emitted("update-links-on-rename")).toEqual([[false]]);
    wrapper.unmount();
  });
});
