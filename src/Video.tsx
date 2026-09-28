import React from "react";
import { AbsoluteFill, Audio, interpolate, Sequence, staticFile, useCurrentFrame } from "remotion";
import { Background } from "./components/Background";
import { NameTag, Yukkuri } from "./components/Characters";
import { MemePop } from "./components/Meme";
import { Subtitle } from "./components/Subtitle";
import { Logo, SceneTitle } from "./components/Ui";
import type { PromoProps } from "./Root";
import { SCENE_VIEWS } from "./scenes";
import { CAST, estimateTimeline, SCENES, SceneTiming } from "./timeline";

export type SceneViewProps = { starts: number[]; frames: number };

const Stage: React.FC<{ children: React.ReactNode; frames: number }> = ({ children, frames }) => {
  const f = useCurrentFrame();
  const o = interpolate(f, [0, 10, frames - 8, frames], [0, 1, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  const x = interpolate(f, [0, 12], [80, 0], { extrapolateRight: "clamp" });
  return <AbsoluteFill style={{ opacity: o, transform: `translateX(${x}px)` }}>{children}</AbsoluteFill>;
};

const SceneBlock: React.FC<{ index: number; timing: SceneTiming; memes: Record<string, string> }> = ({ index, timing, memes }) => {
  const scene = SCENES[index];
  const View = SCENE_VIEWS[scene.id];
  const starts = timing.lines.map((l) => l.from);
  const f = useCurrentFrame();
  const current = timing.lines.findIndex((l) => f >= l.from && f < l.from + l.frames);
  const speaker = current >= 0 ? scene.lines[current].who : null;

  return (
    <AbsoluteFill>
      <Stage frames={timing.frames}>{View && <View starts={starts} frames={timing.frames} />}</Stage>
      <SceneTitle title={scene.title} index={index} total={SCENES.length} />
      <Audio src={staticFile("sfx/whoosh.wav")} volume={0.5} />

      {/* 角色常驻：说话的一方弹跳 + 张嘴 */}
      <Yukkuri who="dong" speaking={speaker === "dong"} x={40} y={690} size={250} />
      <Yukkuri who="hu" speaking={speaker === "hu"} x={1630} y={690} size={250} flip />
      <NameTag text={CAST.dong.name} color={CAST.dong.color} x={110} y={928} active={speaker === "dong"} />
      <NameTag text={CAST.hu.name} color={CAST.hu.color} x={1705} y={928} active={speaker === "hu"} />

      {timing.lines.map((lt, i) => {
        const line = scene.lines[i];
        const c = CAST[line.who];
        return (
          <Sequence key={i} from={lt.from} durationInFrames={lt.frames} layout="none">
            {lt.file && <Audio src={staticFile(lt.file)} />}
            <Subtitle name={c.name} color={c.color} text={line.text} frames={lt.frames} />
            {line.meme && (
              <>
                <MemePop memeKey={line.meme} file={memes[line.meme]} side={line.who === "dong" ? "left" : "right"} frames={lt.frames} />
                <Audio src={staticFile("sfx/pop.wav")} volume={0.6} />
              </>
            )}
          </Sequence>
        );
      })}
    </AbsoluteFill>
  );
};

export const Promo: React.FC<PromoProps> = ({ timeline, memes, voiced }) => {
  const tl = timeline ?? estimateTimeline();
  return (
    <AbsoluteFill style={{ backgroundColor: "#0b1426" }}>
      <Background />
      {voiced && <Audio src={staticFile("sfx/bgm.wav")} loop volume={0.06} />}
      <Logo />
      {tl.scenes.map((s, i) => (
        <Sequence key={s.id} from={s.from} durationInFrames={s.frames} name={SCENES[i].title}>
          <SceneBlock index={i} timing={s} memes={memes} />
        </Sequence>
      ))}
    </AbsoluteFill>
  );
};
