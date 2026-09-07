// Orthographic oblique camera: fixed -45° yaw / ~35.264° elevation, wheel
// zoom via the ortho frustum scale, drag pan across the ground plane. The
// camera never rotates after setup — only the ground focus point and the
// frustum size change, so the map's orientation stays stable for tools.

import { Axis, Camera, FreeCamera, Scene, Vector3 } from "@babylonjs/core";

// Camera sits along the isometric diagonal: azimuth 45° (alpha -45° in
// arc-camera terms), elevation 35.264° (atan(1/√2) above the horizon).
const EYE_OFFSET = new Vector3(1, 1, -1).normalize();
const EYE_DISTANCE = 400;
const MIN_VIEW_HEIGHT = 16;
const MAX_VIEW_HEIGHT = 220;
const ZOOM_STEP = 1.1;

export interface CityCamera {
  camera: FreeCamera;
  viewHeight: number;
  dispose(): void;
}

export function createCamera(
  scene: Scene,
  canvas: HTMLCanvasElement,
  focus: Vector3,
  initialViewHeight = 96,
): CityCamera {
  const engine = scene.getEngine();
  const camera = new FreeCamera("camera", Vector3.Zero(), scene);
  camera.mode = Camera.ORTHOGRAPHIC_CAMERA;
  camera.minZ = 1;
  camera.maxZ = 4000;

  let viewHeight = initialViewHeight;
  const target = focus.clone();

  const applyFrustum = (): void => {
    const aspect = engine.getRenderWidth() / Math.max(1, engine.getRenderHeight());
    const halfH = viewHeight / 2;
    const halfW = halfH * aspect;
    camera.orthoLeft = -halfW;
    camera.orthoRight = halfW;
    camera.orthoTop = halfH;
    camera.orthoBottom = -halfH;
  };

  const place = (): void => {
    camera.position.copyFrom(target).addInPlace(EYE_OFFSET.scale(EYE_DISTANCE));
    camera.setTarget(target);
  };

  applyFrustum();
  place();

  // Screen pixels → ground-plane world units for the current frustum.
  const worldPerPixel = (): { x: number; y: number } => {
    const rect = canvas.getBoundingClientRect();
    return {
      x: (camera.orthoRight! - camera.orthoLeft!) / Math.max(1, rect.width),
      y: (camera.orthoTop! - camera.orthoBottom!) / Math.max(1, rect.height),
    };
  };

  const groundDir = (local: Vector3): Vector3 => {
    const flat = new Vector3(local.x, 0, local.z);
    const length = Math.hypot(flat.x, flat.z);
    return length > 0 ? flat.scaleInPlace(1 / length) : flat;
  };

  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  const onPointerDown = (ev: PointerEvent): void => {
    dragging = true;
    lastX = ev.clientX;
    lastY = ev.clientY;
  };

  const onPointerMove = (ev: PointerEvent): void => {
    if (!dragging) return;
    const wpp = worldPerPixel();
    const dx = (ev.clientX - lastX) * wpp.x;
    const dy = (ev.clientY - lastY) * wpp.y;
    lastX = ev.clientX;
    lastY = ev.clientY;
    // Drag right → world slides right → focus moves left; drag down → world
    // slides down-screen → focus moves along the view direction.
    const right = groundDir(camera.getDirection(Axis.X));
    const forward = groundDir(camera.getDirection(Axis.Z));
    target.addInPlace(right.scaleInPlace(-dx)).addInPlace(forward.scaleInPlace(dy));
    place();
  };

  const onPointerUp = (): void => {
    dragging = false;
  };

  const onWheel = (ev: WheelEvent): void => {
    ev.preventDefault();
    const factor = ev.deltaY > 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
    viewHeight = Math.min(MAX_VIEW_HEIGHT, Math.max(MIN_VIEW_HEIGHT, viewHeight * factor));
    applyFrustum();
  };

  const onResize = (): void => applyFrustum();

  canvas.addEventListener("pointerdown", onPointerDown);
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  engine.onResizeObservable.add(onResize);

  return {
    camera,
    get viewHeight(): number {
      return viewHeight;
    },
    dispose(): void {
      engine.onResizeObservable.removeCallback(onResize);
      canvas.removeEventListener("wheel", onWheel);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerdown", onPointerDown);
    },
  };
}
