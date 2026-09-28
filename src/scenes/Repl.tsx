import React from "react";
import { spring, useCurrentFrame, useVideoConfig } from "remotion";
import { Cursor } from "../components/Terminal";
import { C, FONT, MONO } from "../theme";
import type { SceneViewProps } from "../Video";

const MEMBERS = ["audioInfo", "decodeAudio", "formats", "mp4Info", "mp4Sample", "mp4Samples", "play", "stop", "videoEncode"];
const DOCS: Record<string, [string, string]> = {
  audioInfo: ["audioInfo(bytes, format?)", "读取音频容器/编码/采样率/声道，不解码 PCM"],
  decodeAudio: ["decodeAudio(bytes, format?)", "整段解码为 { sampleRate, channels, duration, data: Float32Array }"],
  formats: ["formats()", "列出支持的音视频格式及编/解码能力"],
  mp4Info: ["mp4Info(bytes)", "MP4 解复用：轨道、编码、时长"],
};

// 真实 REPL 高亮口径的简化版
const hl = (s: string) =>
  s.split(/(\b(?:await|const|let)\b|"[^"]*"|\.\w+)/g).map((t, i) =>
    /^(await|const|let)$/.test(t) ? <span key={i} style={{ color: "#c792ea" }}>{t}</span>
      : /^"/.test(t) ? <span key={i} style={{ color: "#a5e075" }}>{t}</span>
      : /^\.\w+/.test(t) ? <span key={i} style={{ color: "#82aaff" }}>{t}</span>
      : <span key={i}>{t}</span>,
  );

const Typed: React.FC<{ text: string; at: number; cps?: number }> = ({ text, at, cps = 0.9 }) => {
  const f = useCurrentFrame();
  const n = Math.max(0, Math.min(text.length, Math.floor((f - at) * cps)));
  return (
    <>
      {hl(text.slice(0, n))}
      {n < text.length && f >= at && <Cursor />}
    </>
  );
};

export const Repl: React.FC<SceneViewProps> = ({ starts }) => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const pop = spring({ frame: f - starts[0], fps, config: { damping: 14 } });
  const P = <span style={{ color: C.ice }}>❄&gt; </span>;

  const typeAt = starts[2] + 6;
  const partial = "WinterJS.media.";
  const menuAt = typeAt + Math.ceil(partial.length / 0.9) + 8;
  const sel = Math.min(1 + Math.floor(Math.max(0, f - menuAt) / 18), 3) % MEMBERS.length;
  const showMenu = f >= menuAt && f < starts[4];
  const docCmd = ".doc WinterJS.media.decodeAudio";
  const docOut = starts[4] + Math.ceil(docCmd.length / 0.9) + 6;
  const tla = 'await fetch("data:text/plain,hi").then((r) => r.text())';
  const tlaOut = starts[5] + Math.ceil(tla.length / 0.9) + 6;

  const rows: React.ReactNode[] = [];
  if (f >= starts[1]) {
    rows.push(<div key="sh"><span style={{ color: C.good }}>$ </span><Typed text="winterjs" at={starts[1]} /></div>);
    if (f >= starts[1] + 20)
      rows.push(<div key="ban" style={{ color: C.dim }}>❄️ WinterJS 26.9.27 · SpiderMonkey · 输入 .help 查看帮助</div>);
  }
  if (f >= starts[2] && f < starts[4]) {
    rows.push(<div key="c1">{P}<Typed text={partial} at={typeAt} />{showMenu && <span>{MEMBERS[sel]}<Cursor /></span>}</div>);
  }
  if (f >= starts[4]) {
    rows.push(<div key="c1d">{P}WinterJS.media.{MEMBERS[sel]}</div>);
    rows.push(<div key="d0">{P}<Typed text={docCmd} at={starts[4]} /></div>);
    if (f >= docOut) {
      rows.push(
        <div key="d1" style={{ borderLeft: `4px solid ${C.gold}`, paddingLeft: 18, margin: "6px 0", color: "#e6edf7", fontFamily: FONT, fontSize: 26, lineHeight: 1.5 }}>
          <div style={{ color: C.gold, fontWeight: 900, fontSize: 30 }}>WinterJS.media.decodeAudio</div>
          <div>把整个音频文件解码为 {"{ format, codec, sampleRate, channels, duration, data }"}，data 为交错的 Float32 PCM。</div>
          <div style={{ color: C.ice, fontFamily: MONO }}>Syntax: WinterJS.media.decodeAudio(bytes, format?)</div>
          <div style={{ color: C.dim }}>format：mp3 / wav / flac / ogg / m4a / aac / aiff / caf / alac / mka，省略则自动嗅探</div>
        </div>,
      );
    }
  }
  if (f >= starts[5]) {
    rows.push(<div key="t0">{P}<Typed text={tla} at={starts[5]} /></div>);
    if (f >= tlaOut) rows.push(<div key="t1" style={{ color: "#a5e075" }}>'hi'</div>);
    if (f >= tlaOut + 20) rows.push(<div key="m0">{P}<Typed text="function greet(name," at={tlaOut + 20} /></div>);
    if (f >= tlaOut + 50) rows.push(<div key="m1"><span style={{ color: C.dim }}>.. </span><Typed text={'  return `hi ${name}` }'} at={tlaOut + 50} /></div>);
  }

  return (
    <div
      style={{
        position: "absolute", left: 90, top: 110, width: 1740, height: 640, borderRadius: 18,
        background: "rgba(6,12,24,0.95)", border: `2px solid ${C.panelBorder}`, transform: `scale(${pop})`,
        boxShadow: "0 24px 60px rgba(0,0,0,0.5)", overflow: "hidden",
      }}
    >
      <div style={{ height: 46, display: "flex", alignItems: "center", gap: 10, padding: "0 18px", background: "rgba(255,255,255,0.06)" }}>
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => <div key={c} style={{ width: 16, height: 16, borderRadius: 8, background: c }} />)}
        <div style={{ marginLeft: 12, color: C.dim, fontFamily: FONT, fontSize: 22 }}>winterjs — REPL</div>
      </div>
      <div style={{ padding: "16px 26px", fontFamily: MONO, fontSize: 30, lineHeight: 1.55, color: "#fff" }}>{rows.slice(-9)}</div>

      {/* 补全菜单 + 右侧文档面板（irb 式） */}
      {showMenu && (
        <div style={{ position: "absolute", left: 340, top: 190, display: "flex", gap: 0, fontFamily: MONO, fontSize: 27 }}>
          <div style={{ background: "#16233d", border: `2px solid ${C.ice}`, borderRadius: "12px 0 0 12px", padding: "6px 0", minWidth: 280 }}>
            {MEMBERS.map((m, i) => (
              <div key={m} style={{ padding: "3px 20px", background: i === sel ? C.ice : "transparent", color: i === sel ? "#0b1426" : "#dfe9f7", fontWeight: i === sel ? 800 : 400 }}>
                {m}
              </div>
            ))}
          </div>
          <div style={{ background: "#0f1a2e", border: `2px solid ${C.ice}`, borderLeft: "none", borderRadius: "0 12px 12px 0", padding: "16px 24px", width: 640, fontFamily: FONT }}>
            <div style={{ fontFamily: MONO, color: C.gold, fontSize: 28, fontWeight: 800 }}>{(DOCS[MEMBERS[sel]] ?? [MEMBERS[sel] + "()"])[0]}</div>
            <div style={{ color: "#dfe9f7", fontSize: 26, marginTop: 10, lineHeight: 1.5 }}>{(DOCS[MEMBERS[sel]] ?? ["", ""])[1]}</div>
            <div style={{ color: C.dim, fontSize: 22, marginTop: 14 }}>Tab 切换 · .doc 看全文</div>
          </div>
        </div>
      )}
    </div>
  );
};
