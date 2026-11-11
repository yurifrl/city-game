// Wire contract with the server's seat (crates/server/src/seat/mod.rs).
// The viewer never interprets the game: frames in, raw input out.

export interface FrameMeta {
  seq: number;
  format: "jpeg";
  w: number;
  h: number;
}

export interface Frame {
  meta: FrameMeta;
  jpeg: Blob;
}

export type ServerText =
  | { type: "caption"; text: string }
  | { type: "error"; message: string };

export type ViewerInput =
  | { type: "pointer"; action: "move" | "down" | "up"; x: number; y: number; button: number }
  | { type: "wheel"; delta_y: number }
  | { type: "key"; code: string };

/** `u32 BE meta length | meta JSON | JPEG bytes`. Throws on a malformed frame. */
export function parseFrame(data: ArrayBuffer): Frame {
  const view = new DataView(data);
  if (data.byteLength < 4) throw new Error("frame shorter than its length prefix");
  const metaLength = view.getUint32(0, false);
  if (4 + metaLength > data.byteLength) throw new Error("frame meta length exceeds frame");
  const meta = JSON.parse(new TextDecoder().decode(new Uint8Array(data, 4, metaLength))) as FrameMeta;
  if (meta.format !== "jpeg") throw new Error(`unsupported frame format ${String(meta.format)}`);
  return { meta, jpeg: new Blob([new Uint8Array(data, 4 + metaLength)], { type: "image/jpeg" }) };
}

export function parseServerText(text: string): ServerText {
  const value = JSON.parse(text) as { type?: string };
  if (value.type === "caption" || value.type === "error") return value as ServerText;
  throw new Error(`unknown server message ${String(value.type)}`);
}

/** Maps a client-space point on a canvas to frame pixels. */
export function toFramePoint(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  frame: { w: number; h: number },
): { x: number; y: number } {
  return {
    x: ((clientX - rect.left) / rect.width) * frame.w,
    y: ((clientY - rect.top) / rect.height) * frame.h,
  };
}

/** Keys the server understands; everything else keeps its browser meaning. */
const FORWARDED_KEYS = /^(Digit[1-9]|KeyR|KeyX|Delete|Escape)$/;
export function isForwardedKey(code: string): boolean {
  return FORWARDED_KEYS.test(code);
}
