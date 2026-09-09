import raw from "./data.json";

export interface Entry { name: string; signature: string; returns: string; doc: string }
export interface Method { name: string; signature: string; returns: string; doc: string }
export interface Attr { name: string; type: string; doc: string }
export interface TypeDoc { name: string; union: string; make: string; doc: string; attrs: Attr[]; methods: Method[] }
export interface Param { name: string; type: string; default: string; doc: string }
export interface Returns { type: string; doc: string }
export interface LibItem { name: string; category: string; isFunc: boolean; signature: string; call: string; summary: string; params: Param[]; returns: Returns }
/** 標準ライブラリ 1 つ。Rust のものは entries、.moph のものは items */
export interface Lib { name: string; path: string; entries: Entry[]; items: LibItem[] }
export interface MediaInfo { width: number; height: number; fps: number; seconds: number }
export interface Sample { name: string; desc: string; code: string; length: number; media: MediaInfo | null; trim: string; listed: boolean }
export interface Doc { path: string; key: string; title: string; text: string }
export interface Example { name: string; code: string }
export interface Data {
  builtins: Entry[];
  types: TypeDoc[];
  libs: Lib[];
  samples: Sample[];
  docs: Doc[];
  examples: Example[];
}

export const data: Data = raw as Data;
