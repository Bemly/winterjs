import React from "react";
import { random, useCurrentFrame } from "remotion";
import { CodePanel } from "../components/Code";
import { BigText, Card, Chip, PopIn } from "../components/Ui";
import { C, FONT, MONO } from "../theme";
import type { SceneViewProps } from "../Video";

const MEDIA_CODE = `const bytes = readFileSync("song.flac");
const pcm = WinterJS.media.decodeAudio(bytes);
// { sampleRate: 44100, channels: 2, data: Float32Array }

const id = WinterJS.media.play(pcm, { volume: 0.8 });

const ivf = WinterJS.media.videoEncode(
  { data: rgbaFrames, width: 640, height: 360, count: 60 },
  { speed: 10 },
); // → AV1 (IVF)

const info = WinterJS.media.mp4Info(mp4Bytes);`;

const IMG_FORMATS = ["PNG", "JPEG", "GIF", "WebP", "TIFF", "TGA", "BMP", "ICO", "HDR", "EXR", "PNM", "farbfeld", "QOI", "SVG", "JPEG XL"];
const TOOLS = ["semver", "yaml", "jsonc", "qrcode", "ip", "shlex", "spdx", "git", "graph", "transpile", "mime", "cookie", "hex", "time", "retry", "log"];

const Wave: React.FC<{ f: number }> = ({ f }) => {
  const pts = Array.from({ length: 120 }, (_, i) => {
    const x = i * 6.5;
    const amp = 70 * (0.4 + 0.6 * Math.abs(Math.sin(i / 9 + f / 10))) * Math.sin(i / 2.2 + f / 3);
    return `${x},${90 + amp}`;
  }).join(" ");
  return (
    <svg width={780} height={180}>
      <polyline points={pts} fill="none" stroke={C.ice} strokeWidth={4} />
    </svg>
  );
};

const QR: React.FC<{ size: number }> = ({ size }) => {
  const n = 25;
  const c = size / n;
  const finder = (x: number, y: number) => x < 7 && y < 7 || x >= n - 7 && y < 7 || x < 7 && y >= n - 7;
  const cells: React.ReactNode[] = [];
  for (let y = 0; y < n; y++)
    for (let x = 0; x < n; x++) {
      let on: boolean;
      if (finder(x, y)) {
        const lx = x < 7 ? x : x - (n - 7);
        const ly = y < 7 ? y : y - (n - 7);
        on = lx === 0 || lx === 6 || ly === 0 || ly === 6 || (lx >= 2 && lx <= 4 && ly >= 2 && ly <= 4);
      } else on = random(`qr${x}-${y}`) > 0.5;
      if (on) cells.push(<rect key={`${x}-${y}`} x={x * c} y={y * c} width={c} height={c} fill="#0b1426" />);
    }
  return (
    <svg width={size} height={size} style={{ background: "#fff", padding: 12, borderRadius: 12 }}>
      {cells}
    </svg>
  );
};

export const Native: React.FC<SceneViewProps> = ({ starts }) => {
  const f = useCurrentFrame();
  const mediaPhase = f >= starts[2] && f < starts[7];
  const imgPhase = f >= starts[7] && f < starts[9];
  const toolPhase = f >= starts[9];
  return (
    <>
      {f < starts[2] && (
        <div style={{ position: "absolute", left: 0, right: 0, top: 170, display: "flex", flexDirection: "column", alignItems: "center" }}>
          <PopIn at={starts[0]} from="zoom" style={{ position: "relative" }}>
            <BigText size={130} color={C.ice}>WinterJS.*</BigText>
          </PopIn>
          <div style={{ marginTop: 30, width: 1400, textAlign: "center" }}>
            {["media", "image", "semver", "yaml", "qrcode", "git", "transpile", "fs", "net", "…"].map((t, i) => (
              <PopIn key={t} at={starts[0] + 10 + i * 4} from="zoom" style={{ position: "relative", display: "inline-block" }}>
                <Chip size={40} color={t === "media" ? C.gold : C.ice}>{t}</Chip>
              </PopIn>
            ))}
          </div>
        </div>
      )}

      {mediaPhase && (
        <>
          <CodePanel file="media.js" code={MEDIA_CODE} x={80} y={120} w={1000} reveal={2 + (f - starts[2]) / 6} fontSize={24} accent={C.gold} />
          <div style={{ position: "absolute", left: 1110, top: 120, width: 740, fontFamily: FONT, color: C.text }}>
            <PopIn at={starts[2]} style={{ position: "relative" }}>
              <Card accent={C.ice} style={{ padding: "12px 20px" }}>
                <div style={{ fontSize: 28, fontWeight: 900, color: C.ice }}>🎵 decodeAudio → Float32 PCM</div>
                <div style={{ transform: "scale(0.9)", transformOrigin: "left" }}><Wave f={f} /></div>
                <div style={{ fontSize: 22, color: C.dim }}>MP3 · FLAC · OGG · WAV · M4A · AAC · AIFF · CAF · ALAC · MKA</div>
              </Card>
            </PopIn>
            {f >= starts[3] && (
              <PopIn at={starts[3]} style={{ position: "relative", marginTop: 16 }}>
                <Card accent={C.good} style={{ padding: "12px 20px", fontSize: 28, fontWeight: 800 }}>
                  🔊 play() 后台线程放音 {"◗".repeat(1 + (Math.floor(f / 8) % 3))}
                </Card>
              </PopIn>
            )}
            {f >= starts[4] && (
              <PopIn at={starts[4]} style={{ position: "relative", marginTop: 16 }}>
                <Card accent={C.fox} style={{ padding: "12px 20px" }}>
                  <div style={{ fontSize: 28, fontWeight: 900, color: C.fox }}>🎞️ RGBA 帧 → AV1</div>
                  <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                    {Array.from({ length: 6 }, (_, i) => (
                      <div key={i} style={{ width: 96, height: 60, borderRadius: 6, border: "3px solid #fff3", background: `hsl(${(f * 3 + i * 40) % 360},70%,55%)` }} />
                    ))}
                  </div>
                  <div style={{ fontFamily: MONO, fontSize: 22, color: C.dim, marginTop: 8 }}>out.ivf · "DKIF" · av01</div>
                </Card>
              </PopIn>
            )}
            {f >= starts[5] && (
              <PopIn at={starts[5]} style={{ position: "relative", marginTop: 16 }}>
                <Card accent={C.ice} style={{ padding: "12px 20px", fontFamily: MONO, fontSize: 24 }}>
                  <div style={{ color: C.ice, fontFamily: FONT, fontWeight: 900, fontSize: 28 }}>📦 mp4Info / mp4Samples</div>
                  <div>track#1  audio  flac  44100Hz</div>
                  <div>samples → mp4Sample(i)</div>
                </Card>
              </PopIn>
            )}
          </div>
        </>
      )}

      {imgPhase && (
        <>
          <PopIn at={starts[7]} style={{ left: 0, right: 0, top: 120, textAlign: "center" }}>
            <BigText size={64} color={C.gold}>🖼️ WinterJS.image · 15 种格式</BigText>
          </PopIn>
          {IMG_FORMATS.map((t, i) => (
            <PopIn key={t} at={starts[7] + 10 + i * 3} from="zoom" style={{ left: 190 + (i % 5) * 310, top: 260 + Math.floor(i / 5) * 140 }}>
              <div style={{ width: 280, height: 110, borderRadius: 18, display: "flex", alignItems: "center", justifyContent: "center", fontFamily: FONT, fontWeight: 900, fontSize: 40, color: "#0b1426", background: `hsl(${200 + i * 11},80%,${t === "JPEG XL" || t === "SVG" ? 70 : 82}%)`, border: "4px solid #0b1426" }}>
                {t}
              </div>
            </PopIn>
          ))}
          {f >= starts[8] && (
            <PopIn at={starts[8]} from="zoom" style={{ left: 0, right: 0, top: 700, textAlign: "center" }}>
              <BigText size={50} color={C.good}>全部内置 · 无需 npm install</BigText>
            </PopIn>
          )}
        </>
      )}

      {toolPhase && (
        <>
          <div style={{ position: "absolute", left: 90, top: 140, width: 1100 }}>
            {TOOLS.map((t, i) => (
              <PopIn key={t} at={starts[9] + i * 3} from="zoom" style={{ position: "relative", display: "inline-block" }}>
                <Chip size={36} color={t === "qrcode" ? C.gold : C.ice}>WinterJS.{t}</Chip>
              </PopIn>
            ))}
          </div>
          {f >= starts[10] && (
            <PopIn at={starts[10]} from="zoom" style={{ left: 1300, top: 150 }}>
              <div style={{ textAlign: "center" }}>
                <QR size={400} />
                <div style={{ fontFamily: MONO, fontSize: 28, color: C.gold, marginTop: 12 }}>WinterJS.qrcode("hi")</div>
              </div>
            </PopIn>
          )}
        </>
      )}
    </>
  );
};
