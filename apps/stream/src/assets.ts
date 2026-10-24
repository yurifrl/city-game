// Static asset loading for the viewer page (templates live in src/public/).

/** Raw viewer.html template; __SEAT_ID__/__WIDTH__/__HEIGHT__/__DPR__ tokens. */
export async function loadViewerTemplate(): Promise<string> {
  return Bun.file(new URL('./public/viewer.html', import.meta.url)).text();
}
