import React from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { CodePanel } from "../components/Code";
import { schedule, Terminal, typedFrames } from "../components/Terminal";
import { BigText, Card, PopIn } from "../components/Ui";
import { C, MONO } from "../theme";
import type { SceneViewProps } from "../Video";

const PROJECTS = [
  { tool: "yarn", file: "yarn.lock", color: "#2c8ebb" },
  { tool: "pnpm", file: "pnpm-lock.yaml", color: "#f9ad00" },
  { tool: "bun", file: "bun.lock", color: "#fbf0df" },
  { tool: "deno", file: "deno.json", color: "#e8eef7" },
];

const BUN_CODE = `import { Database } from "bun:sqlite";
const db = new Database("app.db");

Bun.serve({
  port: 3000,
  fetch(req) {
    const row = db.query("select 42 as n").get();
    return Response.json(row);
  },
});`;

const DENO_CODE = `// main.ts
const cfg: string = await Deno.readTextFile("./config.json");

Deno.serve({ port: 8000 }, (_req: Request) =>
  new Response(cfg, {
    headers: { "content-type": "application/json" },
  }),
);`;

export const Takeover: React.FC<SceneViewProps> = ({ starts }) => {
  const f = useCurrentFrame();
  const flow = interpolate(f, [starts[1], starts[1] + 30], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  const cmdDev = "winterjs -r dev";
  const devItems = schedule(starts, [
    [2, 8, { kind: "cmd", text: cmdDev }],
    [2, 8 + typedFrames(cmdDev), { kind: "out", text: "$ vite   ← scripts.dev", color: C.dim }],
    [2, 14 + typedFrames(cmdDev), { kind: "out", text: "  VITE  ready", color: C.vue }],
    [6, 6, { kind: "cmd", text: "winterjs -t" }],
    [6, 6 + typedFrames("winterjs -t"), { kind: "out", text: "✓ 自动发现测试文件 · 全部通过", color: C.good }],
    [6, 30, { kind: "cmd", text: "winterjs -r test   # → vitest" }],
    [6, 60, { kind: "out", text: " ✓ src/App.spec.ts", color: C.good }],
  ]);
  const showCode = f >= starts[3] && f < starts[6];
  return (
    <>
      {PROJECTS.map((p, i) => (
        <PopIn key={p.tool} at={starts[0] + i * 6} from="left" style={{ left: 90, top: 130 + i * 140, width: 420 }}>
          <Card accent={p.color} style={{ padding: "14px 24px" }}>
            <div style={{ fontSize: 36, fontWeight: 900, color: p.color }}>{p.tool} 项目</div>
            <div style={{ fontSize: 24, fontFamily: MONO, color: C.dim }}>{p.file} · node_modules/</div>
          </Card>
        </PopIn>
      ))}
      {/* 流入箭头 */}
      {f >= starts[1] && (
        <svg width={1920} height={1080} style={{ position: "absolute" }}>
          {PROJECTS.map((p, i) => {
            const y = 190 + i * 140;
            return (
              <line key={p.tool} x1={520} y1={y} x2={520 + 140 * flow} y2={y + (400 - y) * flow} stroke={C.ice} strokeWidth={5} strokeDasharray="14 10" strokeDashoffset={-f * 2} />
            );
          })}
        </svg>
      )}
      <PopIn at={starts[1] + 10} from="zoom" style={{ left: 640, top: 320, width: 330 }}>
        <Card accent={C.ice} glow style={{ textAlign: "center" }}>
          <div style={{ fontSize: 80 }}>❄️</div>
          <BigText size={44}>直接接管</BigText>
          <div style={{ fontSize: 24, color: C.dim }}>不重装 · 不迁移</div>
        </Card>
      </PopIn>

      {showCode && f < starts[4] && <CodePanel file="server.js  (Bun 项目)" code={BUN_CODE} x={1010} y={130} w={820} reveal={(f - starts[3]) / 3} fontSize={26} accent="#fbf0df" />}
      {showCode && f >= starts[4] && <CodePanel file="main.ts  (Deno 项目)" code={DENO_CODE} x={1010} y={130} w={820} reveal={(f - starts[4]) / 3} fontSize={26} accent="#e8eef7" />}
      {f >= starts[5] && f < starts[6] && (
        <PopIn at={starts[5]} from="zoom" style={{ left: 1010, top: 520, width: 820 }}>
          <div style={{ textAlign: "center" }}>
            <BigText size={70} color={C.gold}>零迁移成本！</BigText>
          </div>
        </PopIn>
      )}
      {(f < starts[3] || f >= starts[6]) && f >= starts[2] && (
        <Terminal items={devItems} x={1010} y={140} w={820} h={480} fontSize={28} title="~/old-project" />
      )}
    </>
  );
};
