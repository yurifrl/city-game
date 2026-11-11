<script lang="ts">
  import { onMount } from "svelte";
  import { Stream, type ConnectionState } from "./stream";
  import { isForwardedKey, toFramePoint, type ViewerInput } from "./protocol";

  let canvas: HTMLCanvasElement;
  let caption = $state("");
  let connection = $state<ConnectionState>("connecting");
  let detail = $state("");
  let frameSize = { w: 1280, h: 720 };

  async function streamUrl(): Promise<string> {
    let seed = sessionStorage.getItem("city-seed");
    if (!seed) {
      const response = await fetch("/api/seed");
      if (!response.ok) throw new Error(`seed request failed: ${response.status}`);
      seed = ((await response.json()) as { seed: string }).seed;
      sessionStorage.setItem("city-seed", seed);
    }
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    return `${scheme}://${location.host}/stream?seed=${encodeURIComponent(seed)}`;
  }

  onMount(() => {
    const context = canvas.getContext("2d");
    if (!context) throw new Error("2D canvas unavailable");

    // Only the newest frame matters: if decoding falls behind, older frames
    // are dropped rather than queued.
    let pending: Blob | undefined;
    let decoding = false;
    const drawLatest = async () => {
      if (decoding) return;
      decoding = true;
      while (pending) {
        const blob = pending;
        pending = undefined;
        const bitmap = await createImageBitmap(blob);
        if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
        }
        context.drawImage(bitmap, 0, 0);
        bitmap.close();
      }
      decoding = false;
    };

    const stream = new Stream(streamUrl, {
      onFrame(frame) {
        frameSize = frame.meta;
        pending = frame.jpeg;
        void drawLatest();
      },
      onCaption: (text) => (caption = text),
      onState(next, why) {
        connection = next;
        detail = why ?? "";
      },
    });

    // Pointer moves are coalesced to one per animation frame.
    let queuedMove: ViewerInput | undefined;
    const point = (event: PointerEvent | MouseEvent) =>
      toFramePoint(event.clientX, event.clientY, canvas.getBoundingClientRect(), frameSize);
    const onMove = (event: PointerEvent) => {
      const { x, y } = point(event);
      const first = queuedMove === undefined;
      queuedMove = { type: "pointer", action: "move", x, y, button: 0 };
      if (first) {
        requestAnimationFrame(() => {
          if (queuedMove) stream.send(queuedMove);
          queuedMove = undefined;
        });
      }
    };
    const onButton = (action: "down" | "up") => (event: PointerEvent) => {
      const { x, y } = point(event);
      if (action === "down") canvas.setPointerCapture(event.pointerId);
      stream.send({ type: "pointer", action, x, y, button: event.button });
    };
    const onDown = onButton("down");
    const onUp = onButton("up");
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      stream.send({ type: "wheel", delta_y: event.deltaY });
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat || !isForwardedKey(event.code)) return;
      event.preventDefault();
      stream.send({ type: "key", code: event.code });
    };
    const onContextMenu = (event: Event) => event.preventDefault();

    canvas.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerdown", onDown);
    canvas.addEventListener("pointerup", onUp);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("contextmenu", onContextMenu);
    window.addEventListener("keydown", onKey);
    return () => {
      stream.close();
      canvas.removeEventListener("pointermove", onMove);
      canvas.removeEventListener("pointerdown", onDown);
      canvas.removeEventListener("pointerup", onUp);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("contextmenu", onContextMenu);
      window.removeEventListener("keydown", onKey);
    };
  });
</script>

<main>
  <canvas bind:this={canvas} width="1280" height="720"></canvas>
  <div class="caption" role="status">{caption}</div>
  {#if connection !== "live"}
    <div class="state" role="alert">
      {connection === "connecting" ? "Connecting…" : "Reconnecting…"}
      {#if detail}<small>{detail}</small>{/if}
    </div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    background: #0b0e13;
    color: #dfe6f1;
    font: 14px system-ui, sans-serif;
    overflow: hidden;
  }
  main {
    position: relative;
    height: 100vh;
    display: grid;
    place-items: center;
  }
  canvas {
    max-width: 100vw;
    max-height: 100vh;
    aspect-ratio: 16 / 9;
    touch-action: none;
    cursor: crosshair;
  }
  .caption {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 6px 12px;
    background: rgb(11 14 19 / 0.8);
    pointer-events: none;
  }
  .state {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    padding: 6px 14px;
    border-radius: 6px;
    background: #7a3b1d;
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
</style>
