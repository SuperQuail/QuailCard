import { expect, test } from "vitest";
import { agentDiff } from "./agentDiff";
test("新建、替换和空文件的差异没有丢失正文", () => {
  expect(agentDiff(null, "a\nb")).toEqual([{ kind: "add", text: "a" }, { kind: "add", text: "b" }]);
  expect(agentDiff("a\nb\nc", "a\nx\nc")).toEqual([{ kind: "same", text: "a" }, { kind: "remove", text: "b" }, { kind: "add", text: "x" }, { kind: "same", text: "c" }]);
  expect(agentDiff("", "")).toEqual([{ kind: "same", text: "内容没有变化" }]);
});
