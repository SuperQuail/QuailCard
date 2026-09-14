import { expect, test } from "vitest";
import { isPhonetic, splitPhonetics } from "./phonetic";

test("斜杠音标识别并绑定前面的单词", () => {
  const pieces = splitPhonetics("speak 读 /spiːk/，意思是说。");
  const phonetic = pieces.find(piece => piece.phonetic);
  expect(phonetic).toMatchObject({ text: "/spiːk/", word: "speak" });
  expect(pieces.map(piece => piece.text).join("")).toBe("speak 读 /spiːk/，意思是说。");
});

test("方括号音标同样识别，普通方括号不误判", () => {
  expect(splitPhonetics("derive [dɪˈraɪv]").find(piece => piece.word)?.word).toBe("derive");
  expect(splitPhonetics("参见 [1] 与 [注]").some(piece => piece.phonetic)).toBe(false);
  expect(isPhonetic("/spiːk/")).toBe(true);
  expect(isPhonetic("[1]")).toBe(false);
  expect(isPhonetic("【注】")).toBe(false);
  expect(isPhonetic("中文")).toBe(false);
});
