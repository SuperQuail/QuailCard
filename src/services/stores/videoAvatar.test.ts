import { beforeEach, expect, test, vi } from "vitest";
import * as api from "../../api/video";

vi.mock("../../api/video", () => ({ avatar: vi.fn(), start: vi.fn(), status: vi.fn(), history: vi.fn(), probe: vi.fn() }));
vi.mock("../../api/backend", () => ({ syncNoteIndex: vi.fn(), resolveErrorMessage: String }));
vi.mock("./providerStore", () => ({ activeProviderId: { value: "provider" } }));
vi.mock("./vaultStore", () => ({ vaultPath: { value: null } }));
vi.mock("./noteStore", () => ({ refreshNotes: vi.fn(), notes: { value: [] } }));

import { loadAvatar, videoState } from "./videoStore";

beforeEach(() => {
  vi.resetAllMocks();
  videoState.avatar = "";
  videoState.login = null;
});

// 未登录不发起头像请求，避免游客状态访问 B 站图床。
test("未登录时清空头像且不请求后端", async () => {
  videoState.avatar = "data:image/png;base64,OLD";
  videoState.login = { state: "idle", message: "", image: "", loggedIn: false };
  await loadAvatar();
  expect(api.avatar).not.toHaveBeenCalled();
  expect(videoState.avatar).toBe("");
});

// 头像只是展示信息：失败不能让登录态或其他功能出错。
test("登录后读取代理头像，失败只清空展示", async () => {
  videoState.login = { state: "confirmed", message: "", image: "", loggedIn: true, name: "测试用户" };
  vi.mocked(api.avatar).mockResolvedValue("data:image/png;base64,AAA");
  await loadAvatar();
  expect(videoState.avatar).toBe("data:image/png;base64,AAA");
  vi.mocked(api.avatar).mockRejectedValue(new Error("头像不可用"));
  await loadAvatar();
  expect(videoState.avatar).toBe("");
  expect(videoState.login?.loggedIn).toBe(true);
});
