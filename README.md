# WinterJS 宣传片（B 站）

基于 [Remotion](https://www.remotion.dev/) 的代码化视频工程。主分支 `master` 是 WinterJS 本体；本分支 `promo-video` 是独立的孤儿分支，只放视频工程。

- 时长约 9 分钟，1920×1080 / 30fps，双人（小冬 / 小狐）油库里风对话，烧录字幕 + 外挂 SRT
- 10 个段落：开场 → 底层 SpiderMonkey×mozjs → 横向对比 Node/Deno/Bun → 纵向家谱 → Vue 实战 → 接管 yarn/pnpm/bun/deno 项目 → WinterCG → REPL → `WinterJS.*` 原生 API（多媒体）→ 总结

## 实时预览

```bash
npm install
pip install edge-tts mutagen   # 配音工具
npm run tts                    # 生成配音、时间轴、字幕（按句缓存，改哪句只重合成哪句）
npm run studio                 # 打开 http://localhost:3000 实时预览，改代码热更新
```

不跑 `npm run tts` 也能预览：此时按字数估算节奏，画面静音。

## 出片

- **GitHub Actions**：推送到 `promo-video` 分支即自动渲染（`.github/workflows/render.yml`），
  在该次运行页面底部的 Artifacts 下载 `winterjs-promo`（含 `winterjs-promo.mp4`、`winterjs-promo.srt`、`cover.png`）。
- **本地**：`npm run tts && npm run render` → `out/winterjs-promo.mp4`。

## 改内容

| 要改什么 | 改哪里 |
|---|---|
| 台词 / 字幕 / 发音 / 表情包触发 | `src/script.json`（`text` 是字幕，`say` 是给 TTS 的读法，`meme` 是弹出的表情包） |
| 角色音色 | `src/script.json` 的 `cast`（edge-tts 音色名、音高、语速） |
| 各段画面 | `src/scenes/*.tsx`（画面元素按"第几句开始"对齐） |
| 表情包 | `public/memes/`（`manifest.json` 映射 key → 文件；缺图自动用 emoji 贴纸兜底） |

## 素材与授权

- 表情包：[蓝色大肥鱼](https://蓝色大肥鱼.com/)，只选用详情页标注为 CC0 的作品，逐张出处见 `public/memes/CREDITS.md`
- 配音：Microsoft Edge 在线 TTS（经 `edge-tts`）
- BGM 与音效：`tools/tts.py` 用纯 Python 现场合成，无第三方版权
- 角色形象：原创（SVG 绘制）

## 事实依据（2026-09-28 核对）

- wasmerio/winterjs：GitHub 仓库 2026-03-17 归档，README 注明已弃用
- spiderfire：最近提交 2025-08-27（"Updated SpiderMonkey to 140"）
- GJS：GNOME 官方项目，最新 1.90.0（2026-09-14），定位 GNOME 桌面 JS 绑定
- WinterJS 能力：取自 `master` 分支的 README、`docs/plan3-journal.md`、`sample/` 与 `tests/`
