// @vitest-environment node
import { expect, test } from "vitest";
import { noteContentHash } from "./noteHash";

test("笔记摘要与 Rust SHA-256 契约一致", async () => {
  expect(await noteContentHash("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  expect(await noteContentHash("中文😀\r\n第二行")).toBe(await noteContentHash("中文😀\n第二行"));
});
