export function add(a: number, b: number): number {
  return a + b;
}

export interface Shape {
  kind: string;
  area(): number;
}

export type Id = string | number;

import { readFile } from "node:fs";
import type { Config } from "./config.js";
import { helper } from "./helper.js";
import def, * as ns from "./other.js";

export { helper as util };
export * from "./reexport.js";
export type { Shape as ShapeType } from "./types.js";

export async function load(path: string): Promise<string> {
  const meta = import.meta.url;
  const lazy = await import("./lazy.js");
  return `${meta}:${lazy}`;
}

const _cfg: Config | null = null;
const _rf = readFile;
const _d = def;
const _n = ns;
