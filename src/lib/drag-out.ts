/**
 * Native drag-and-drop helpers — start an OS-level drag from the app so users
 * can drop downloaded files into external editors (CapCut, Premiere, Explorer,
 * Photoshop, etc.) without going through the file picker.
 *
 * Backed by the Crab Nebula `tauri-plugin-drag` plugin which calls the
 * platform-native drag APIs (DoDragDrop on Windows, NSDraggingSession on
 * macOS, GTK on Linux). The browser's HTML5 DataTransfer can't carry actual
 * `CF_HDROP` filesystem paths, which is why we need this plugin.
 */

import { startDrag } from "@crabnebula/tauri-plugin-drag";

/**
 * Begin an OS-level drag containing one local file path. The drag follows the
 * cursor immediately and the drop target receives `CF_HDROP` (Windows) or
 * `NSFilenamesPboardType` (macOS) — i.e., it behaves exactly like a drag
 * originating from File Explorer / Finder.
 *
 * Returns a promise that resolves when the drag completes (drop or cancel).
 * Errors are swallowed: if the plugin is unavailable or the path is invalid,
 * we simply do nothing — the row's normal click handlers still work.
 */
export async function startFileDrag(absolutePath: string): Promise<void> {
  if (!absolutePath) return;
  try {
    await startDrag({ item: [absolutePath], icon: absolutePath });
  } catch (err) {
    console.warn("[drag-out] failed:", err);
  }
}

/**
 * Begin an OS-level drag containing multiple files at once. Editors like
 * CapCut, Premiere, Resolve etc. accept multi-file drops and queue them up
 * in the media bin / timeline. Use the first file as the drag preview icon.
 */
export async function startMultiFileDrag(absolutePaths: string[]): Promise<void> {
  const valid = absolutePaths.filter((p) => p && p.length > 0);
  if (valid.length === 0) return;
  try {
    await startDrag({ item: valid, icon: valid[0] });
  } catch (err) {
    console.warn("[drag-out-multi] failed:", err);
  }
}
