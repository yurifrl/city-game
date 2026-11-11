import { expect, test } from "bun:test";
import { isForwardedKey, parseFrame, parseServerText, toFramePoint } from "../src/protocol";

function framed(meta: object, body: number[]): ArrayBuffer {
  const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
  const out = new Uint8Array(4 + metaBytes.length + body.length);
  new DataView(out.buffer).setUint32(0, metaBytes.length, false);
  out.set(metaBytes, 4);
  out.set(body, 4 + metaBytes.length);
  return out.buffer;
}

test("parses a frame into meta and JPEG bytes", async () => {
  const frame = parseFrame(framed({ seq: 7, format: "jpeg", w: 1280, h: 720 }, [0xff, 0xd8, 1, 2]));
  expect(frame.meta).toEqual({ seq: 7, format: "jpeg", w: 1280, h: 720 });
  expect([...new Uint8Array(await frame.jpeg.arrayBuffer())]).toEqual([0xff, 0xd8, 1, 2]);
});

test("rejects malformed frames instead of drawing garbage", () => {
  expect(() => parseFrame(new ArrayBuffer(2))).toThrow();
  const oversized = new Uint8Array(8);
  new DataView(oversized.buffer).setUint32(0, 999, false);
  expect(() => parseFrame(oversized.buffer)).toThrow();
  expect(() => parseFrame(framed({ seq: 1, format: "png", w: 1, h: 1 }, []))).toThrow();
});

test("server text is a caption or an error, nothing else", () => {
  expect(parseServerText('{"type":"caption","text":"hi"}')).toEqual({ type: "caption", text: "hi" });
  expect(() => parseServerText('{"type":"world"}')).toThrow();
});

test("pointer positions scale from the displayed canvas to frame pixels", () => {
  const rect = { left: 100, top: 50, width: 640, height: 360 };
  expect(toFramePoint(420, 230, rect, { w: 1280, h: 720 })).toEqual({ x: 640, y: 360 });
});

test("only the seat's keys are captured from the browser", () => {
  for (const code of ["Digit1", "Digit9", "KeyR", "KeyX", "Delete", "Escape"]) expect(isForwardedKey(code)).toBe(true);
  for (const code of ["Digit0", "KeyF", "F5", "KeyW"]) expect(isForwardedKey(code)).toBe(false);
});
