import React from "react";
import { spring, useCurrentFrame, useVideoConfig } from "remotion";
import { BigText, Chip, PopIn } from "../components/Ui";
import { C, FONT, MONO } from "../theme";
import type { SceneViewProps } from "../Video";

export const Outro: React.FC<SceneViewProps> = ({ starts }) => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const triple = f >= starts[3];
  return (
    <>
      <div style={{ position: "absolute", left: 0, right: 0, top: 120, display: "flex", flexDirection: "column", alignItems: "center" }}>
        <PopIn at={starts[0]} from="zoom" style={{ position: "relative" }}>
          <BigText size={120}>❄️ Winter<span style={{ color: C.ice }}>JS</span></BigText>
        </PopIn>
        <div style={{ marginTop: 20, textAlign: "center", width: 1500 }}>
          {[
            ["🦊 火狐的引擎", starts[0] + 10],
            ["🦀 Rust 的身体", starts[0] + 20],
            ["Node · Bun · Deno 通吃", starts[0] + 30],
            ["Vue 一条龙", starts[1]],
            ["REPL 带文档", starts[1] + 8],
            ["原生多媒体", starts[1] + 16],
          ].map(([t, at]) => (
            <PopIn key={t as string} at={at as number} from="zoom" style={{ position: "relative", display: "inline-block" }}>
              <Chip size={40} color={(at as number) >= starts[1] ? C.gold : C.ice}>{t}</Chip>
            </PopIn>
          ))}
        </div>
        {f >= starts[2] && (
          <PopIn at={starts[2]} style={{ position: "relative", marginTop: 34 }}>
            <div style={{ fontFamily: MONO, fontSize: 50, color: "#fff", background: "rgba(0,0,0,0.45)", borderRadius: 18, padding: "14px 36px", textAlign: "center", lineHeight: 1.5 }}>
              <div>github.com/<span style={{ color: C.ice }}>Bemly/winterjs</span></div>
              <div style={{ fontSize: 40, color: C.gold }}>winterjs.bemly.moe</div>
            </div>
          </PopIn>
        )}
      </div>
      {triple && (
        <div style={{ position: "absolute", left: 0, right: 0, top: 640, display: "flex", justifyContent: "center", gap: 70 }}>
          {[["👍", "点赞"], ["🪙", "投币"], ["⭐", "收藏"]].map(([e, t], i) => {
            const s = spring({ frame: f - starts[3] - i * 6, fps, config: { damping: 8, stiffness: 200 } });
            return (
              <div key={t} style={{ textAlign: "center", transform: `scale(${s}) translateY(${Math.sin((f + i * 10) / 6) * 6}px)` }}>
                <div style={{ width: 130, height: 130, borderRadius: 65, background: "#fb7299", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 76, border: "5px solid #fff" }}>{e}</div>
                <div style={{ fontFamily: FONT, fontWeight: 900, fontSize: 32, color: "#fff", marginTop: 8 }}>{t}</div>
              </div>
            );
          })}
        </div>
      )}
      <div style={{ position: "absolute", left: 0, right: 0, top: 880, textAlign: "center", fontFamily: FONT, fontSize: 22, color: C.dim, opacity: f >= starts[4] ? 1 : 0 }}>
        表情包：蓝色大肥鱼（CC0） · 配音：Edge TTS · 引擎：Mozilla SpiderMonkey · 本片由 Remotion 渲染
      </div>
    </>
  );
};
