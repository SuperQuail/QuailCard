# Changelog

<!-- qegg:release 0.6.0 -->
## v0.6.0 - 2026-11-03

### Features

- [`a6ed3bf`](https://github.com/SuperQuail/QuailCard/commit/a6ed3bf02d3491ba7001108c6fe2f2245df3c666): 看中文释义逐字母打出英文单词，每个单词重复多遍后进入模糊释义与已完成记录的记忆遍；带连击、手动翻页与自动朗读，练习成绩只做本次统计，不写入复习调度。
- [`2cc4d66`](https://github.com/SuperQuail/QuailCard/commit/2cc4d6606c06586b812c74e4e43750e8076157e8): 打字背诵兼容单词组：单词组编辑区可直接开始练习，范围按「本组 / 全部单词」展示；开始前先落盘并重载卡片，单词组保存后卡片变化会重建词池，避免练到已改或已删的单词。 ([#2](https://github.com/SuperQuail/QuailCard/pull/2) by @Wsrsq)
- [`2cc4d66`](https://github.com/SuperQuail/QuailCard/commit/2cc4d6606c06586b812c74e4e43750e8076157e8): 新增单词组（`.qword`）文件：右键即可创建，按页编写单词并在保存时自动生成单词卡片，音标可根据单词自动查询。 ([#2](https://github.com/SuperQuail/QuailCard/pull/2) by @Wsrsq)

**Full Changelog**: https://github.com/SuperQuail/QuailCard/compare/app-v0.5.0...app-v0.6.0
<!-- qegg:release:end -->

<!-- qegg:release 0.5.0 -->
## v0.5.0 - 2026-10-31

### Features

- [`833baef`](https://github.com/SuperQuail/QuailCard/commit/833baef63be8faa9ad0027a369d3b5d83188c569): 右侧栏新增标签与大纲，frontmatter 只读展示。
- [`c261927`](https://github.com/SuperQuail/QuailCard/commit/c261927d94bf59a113443b20f5c35b09c4032608): 图片支持右键复制、另存为与定位到文件夹。
- [`6089536`](https://github.com/SuperQuail/QuailCard/commit/6089536d9057a9767900837041220b801201db2e): 支持 [[双链]] 与反链面板，改名时自动同步引用。
- [`d7b332b`](https://github.com/SuperQuail/QuailCard/commit/d7b332bb051d102bb26fccdfd365cca53b3a67d6): 删除走系统回收站，还原后卡片与复习记录自动接回。

**Full Changelog**: https://github.com/SuperQuail/QuailCard/compare/app-v0.4.0...app-v0.5.0
<!-- qegg:release:end -->
