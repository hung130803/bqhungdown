// Stub for @tauri-apps/api/event when running in web mode

export type Event<T> = {
  event: string;
  id: number;
  payload: T;
};

export type UnlistenFn = () => void;

export type EventCallback<T> = (event: Event<T>) => void;

export type EventTarget =
  | { kind: "Any" }
  | { kind: "AnyLabel"; label: string }
  | { kind: "App" }
  | { kind: "Window"; label: string }
  | { kind: "Webview"; label: string }
  | { kind: "WebviewWindow"; label: string };

export interface Options {
  target?: string | EventTarget;
}

export enum TauriEvent {
  WINDOW_RESIZED = "tauri://resize",
  WINDOW_MOVED = "tauri://move",
  WINDOW_CLOSE_REQUESTED = "tauri://close-requested",
  DRAG_ENTER = "tauri://drag-enter",
  DRAG_OVER = "tauri://drag-over",
  DRAG_DROP = "tauri://drag-drop",
  DRAG_LEAVE = "tauri://drag-leave",
}

export async function listen<T>(
  _event: string,
  _handler: EventCallback<T>,
  _options?: Options
): Promise<UnlistenFn> {
  return () => {};
}

export async function once<T>(
  _event: string,
  _handler: EventCallback<T>,
  _options?: Options
): Promise<UnlistenFn> {
  return () => {};
}

export async function emit<T>(_event: string, _payload?: T): Promise<void> {
  // noop
}

export async function emitTo<T>(
  _target: EventTarget | string,
  _event: string,
  _payload?: T
): Promise<void> {
  // noop
}
