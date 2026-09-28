import React from "react";
import { useCurrentFrame } from "remotion";
import { schedule, Terminal, typedFrames } from "../components/Terminal";
import { BigText, Card, PopIn } from "../components/Ui";
import { C, FONT, MONO } from "../theme";
import type { SceneViewProps } from "../Video";

const CMD = {
  add: "winterjs -a create-vue",
  create: "winterjs -r node_modules/.bin/create-vue -- hello-vue --ts --router",
  init: "cd hello-vue && winterjs -I -y",
  build: "winterjs -r build",
  dev: "winterjs -r dev",
};

export const VueScene: React.FC<SceneViewProps> = ({ starts }) => {
  const f = useCurrentFrame();
  const items = schedule(starts, [
    [1, 10, { kind: "cmd", text: CMD.add }],
    [1, 10 + typedFrames(CMD.add), { kind: "out", text: "+ create-vue  → node_modules", color: C.good }],
    [2, 6, { kind: "cmd", text: CMD.create }],
    [2, 6 + typedFrames(CMD.create), { kind: "out", text: "Scaffolding project in ./hello-vue..." }],
    [2, 16 + typedFrames(CMD.create), { kind: "out", text: "Done.", color: C.good }],
    [3, 6, { kind: "cmd", text: CMD.init }],
    [3, 6 + typedFrames(CMD.init), { kind: "out", text: "✓ dependencies installed → node_modules", color: C.good }],
    [6, 6, { kind: "cmd", text: CMD.build }],
    [6, 6 + typedFrames(CMD.build), { kind: "out", text: "vite building for production..." }],
    [6, 26 + typedFrames(CMD.build), { kind: "out", text: "✓ built  dist/index.html  dist/assets/*", color: C.good }],
    [7, 6, { kind: "cmd", text: CMD.dev }],
    [7, 6 + typedFrames(CMD.dev), { kind: "out", text: "  VITE  ready", color: C.vue }],
    [7, 12 + typedFrames(CMD.dev), { kind: "out", text: "  ➜  Local:   http://localhost:5173/", color: "#fff" }],
  ]);
  return (
    <>
      <PopIn at={starts[0]} style={{ left: 1230, top: 130, width: 600 }}>
        <Card accent={C.vue} style={{ textAlign: "center" }}>
          <div style={{ fontSize: 110, lineHeight: 1 }}>
            <span style={{ color: C.vue, fontWeight: 900 }}>▼</span>
          </div>
          <BigText size={58} color={C.vue}>Vue 3 + Vite</BigText>
          <div style={{ fontSize: 28, color: C.dim, marginTop: 6 }}>全程只用 winterjs 一个命令</div>
        </Card>
      </PopIn>
      {f >= starts[4] && f < starts[7] && (
        <PopIn at={starts[4]} from="zoom" style={{ left: 1230, top: 470, width: 600 }}>
          <Card accent={C.gold} style={{ fontSize: 40, fontWeight: 900, textAlign: "center", lineHeight: 1.6 }}>
            <div><span style={{ color: "#56627a", textDecoration: "line-through" }}>node</span>　<span style={{ color: "#56627a", textDecoration: "line-through" }}>npm</span></div>
            {f >= starts[5] && <div style={{ color: C.gold, fontSize: 34 }}>.bin 里的 JS 命令由 WinterJS 自己执行</div>}
          </Card>
        </PopIn>
      )}
      {f >= starts[7] && (
        <PopIn at={starts[7] + 30} from="right" style={{ left: 1210, top: 440, width: 640 }}>
          <div style={{ borderRadius: 16, overflow: "hidden", border: "2px solid #ccd6e3", boxShadow: "0 20px 50px rgba(0,0,0,0.5)" }}>
            <div style={{ background: "#e9edf3", padding: "8px 14px", fontFamily: MONO, fontSize: 20, color: "#445" }}>🔒 localhost:5173</div>
            <div style={{ background: "#fff", padding: "34px 30px", fontFamily: FONT }}>
              <div style={{ fontSize: 48, fontWeight: 900, color: C.vue }}>You did it!</div>
              <div style={{ fontSize: 24, color: "#333", marginTop: 8 }}>You’ve successfully created a project with Vite + Vue 3.</div>
              <div style={{ fontSize: 22, color: "#888", marginTop: 14 }}>served by ❄️ WinterJS</div>
            </div>
          </div>
        </PopIn>
      )}
      <Terminal items={items} x={90} y={120} w={1080} h={620} fontSize={27} title="~/projects — winterjs" />
    </>
  );
};
