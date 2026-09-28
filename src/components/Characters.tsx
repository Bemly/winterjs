import React from "react";
import { interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { C } from "../theme";
import type { Who } from "../timeline";

type Props = { who: Who; speaking: boolean; x: number; y: number; size: number; flip?: boolean; mood?: "normal" | "happy" };

/** 油库里风格的"馒头"角色（原创造型）：小冬＝雪团，小狐＝狐火团。 */
export const Yukkuri: React.FC<Props> = ({ who, speaking, x, y, size, flip }) => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const blink = f % 110 > 104;
  const bounce = speaking ? Math.abs(Math.sin(f / 4)) * 10 : Math.sin(f / 30) * 3;
  const mouthOpen = speaking && Math.floor(f / 3) % 3 !== 2;
  const squash = speaking ? 1 + Math.sin(f / 2) * 0.02 : 1;
  const enter = spring({ frame: f, fps, config: { damping: 12 } });
  const body = who === "dong" ? C.ice : C.fox;
  const bodyDeep = who === "dong" ? C.iceDeep : C.foxDeep;
  const dim = speaking ? 1 : 0.72;

  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y - bounce,
        width: size,
        height: size,
        transform: `scale(${enter * (speaking ? 1.06 : 1)}) scaleX(${flip ? -1 : 1}) scaleY(${squash})`,
        transformOrigin: "50% 100%",
        filter: `brightness(${dim}) drop-shadow(0 12px 18px rgba(0,0,0,0.45))`,
      }}
    >
      <svg viewBox="0 0 200 200" width={size} height={size}>
        <defs>
          <radialGradient id={`g-${who}`} cx="40%" cy="35%" r="70%">
            <stop offset="0%" stopColor="#ffffff" stopOpacity="0.55" />
            <stop offset="45%" stopColor={body} />
            <stop offset="100%" stopColor={bodyDeep} />
          </radialGradient>
        </defs>
        {who === "hu" && (
          <>
            {/* 狐耳 + 狐火尾巴 */}
            <path d="M40 70 L55 12 L88 52 Z" fill={C.foxDeep} />
            <path d="M160 70 L145 12 L112 52 Z" fill={C.foxDeep} />
            <path d="M50 58 L57 28 L76 50 Z" fill="#ffe2c4" />
            <path d="M150 58 L143 28 L124 50 Z" fill="#ffe2c4" />
            <path
              d={`M168 150 Q ${200 + Math.sin(f / 6) * 6} 120 185 ${86 + Math.cos(f / 5) * 5} Q 176 118 160 132 Z`}
              fill="#ffcf4a"
              opacity={0.9}
            />
          </>
        )}
        <ellipse cx="100" cy="118" rx="84" ry="72" fill={`url(#g-${who})`} stroke="#1b2740" strokeWidth="4" />
        {who === "dong" && (
          <>
            {/* 雪帽 + 雪花发饰 */}
            <path d="M34 92 Q 100 20 166 92 Q 150 70 100 66 Q 50 70 34 92 Z" fill="#ffffff" stroke="#1b2740" strokeWidth="3" />
            <g transform="translate(150 70)">
              {[0, 60, 120].map((a) => (
                <line key={a} x1={-14} y1={0} x2={14} y2={0} stroke="#bfe6ff" strokeWidth="4" strokeLinecap="round" transform={`rotate(${a + f})`} />
              ))}
            </g>
          </>
        )}
        {who === "hu" && <ellipse cx="100" cy="152" rx="46" ry="26" fill="#fff4e6" />}
        {/* 眼睛 */}
        {[70, 130].map((ex) =>
          blink ? (
            <path key={ex} d={`M${ex - 16} 112 Q ${ex} 120 ${ex + 16} 112`} stroke="#1b2740" strokeWidth="5" fill="none" strokeLinecap="round" />
          ) : (
            <g key={ex}>
              <ellipse cx={ex} cy="110" rx="17" ry="21" fill="#fff" stroke="#1b2740" strokeWidth="3.5" />
              <ellipse cx={ex + 3} cy="113" rx="10" ry="13" fill="#1b2740" />
              <circle cx={ex + 7} cy="106" r="4.5" fill="#fff" />
            </g>
          ),
        )}
        {/* 腮红 */}
        <ellipse cx="46" cy="136" rx="12" ry="6" fill="#ff7aa8" opacity={0.55} />
        <ellipse cx="154" cy="136" rx="12" ry="6" fill="#ff7aa8" opacity={0.55} />
        {/* 嘴 */}
        {mouthOpen ? (
          <path d="M84 140 Q 100 168 116 140 Z" fill="#7a1f35" stroke="#1b2740" strokeWidth="3" />
        ) : (
          <path d="M88 142 Q 100 150 112 142" fill="none" stroke="#1b2740" strokeWidth="4" strokeLinecap="round" />
        )}
      </svg>
    </div>
  );
};

export const NameTag: React.FC<{ text: string; color: string; x: number; y: number; active: boolean }> = ({ text, color, x, y, active }) => {
  const f = useCurrentFrame();
  const o = interpolate(f, [0, 10], [0, 1], { extrapolateRight: "clamp" });
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        padding: "4px 18px",
        borderRadius: 999,
        background: active ? color : "rgba(255,255,255,0.12)",
        color: active ? "#0b1426" : "#dfe9f7",
        fontWeight: 900,
        fontSize: 26,
        opacity: o,
        border: "3px solid #0b1426",
      }}
    >
      {text}
    </div>
  );
};
