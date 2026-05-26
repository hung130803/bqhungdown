// Stub for @tauri-apps/api/core when running in web mode
// On desktop (Tauri), the real @tauri-apps/api/core is used.
// On web, we stub it out so it doesn't crash.

export const SERIALIZE_TO_IPC_FN = "__TAURI_TO_IPC_KEY__";

export function transformCallback<T = unknown>(
  _callback?: (response: T) => void,
  _once?: boolean
): number {
  return 0;
}

export class Channel<T = unknown> {
  id: number = 0;
  onmessage: ((response: T) => void) | null = null;
  [SERIALIZE_TO_IPC_FN](): string {
    return `__CHANNEL__:0`;
  }
  toJSON(): string {
    return this[SERIALIZE_TO_IPC_FN]();
  }
}

export class PluginListener {
  plugin: string;
  event: string;
  channelId: number;
  constructor(plugin: string, event: string, channelId: number) {
    this.plugin = plugin;
    this.event = event;
    this.channelId = channelId;
  }
  unregister(): Promise<void> {
    return Promise.resolve();
  }
}

export async function addPluginListener<T>(
  _plugin: string,
  _event: string,
  _cb: (payload: T) => void
): Promise<PluginListener> {
  return new PluginListener("", "", 0);
}

export type PermissionState = "granted" | "denied" | "prompt" | "prompt-with-rationale";

export async function checkPermissions<T>(_plugin: string): Promise<T> {
  return {} as T;
}

export async function requestPermissions<T>(_plugin: string): Promise<T> {
  return "granted" as unknown as T;
}

export type InvokeArgs = Record<string, unknown> | number[] | ArrayBuffer | Uint8Array;

export interface InvokeOptions {
  headers?: HeadersInit;
}

export async function invoke<T>(
  _cmd: string,
  _args?: InvokeArgs,
  _options?: InvokeOptions
): Promise<T> {
  return Promise.reject(
    new Error(
      "[TauriStub] invoke() called in web mode — this should not happen. Check IS_WEB guards."
    )
  );
}

export function convertFileSrc(_filePath: string, _protocol?: string): string {
  return _filePath;
}

export class Resource {
  #rid: number;
  get rid(): number {
    return this.#rid;
  }
  constructor(rid: number) {
    this.#rid = rid;
  }
  close(): Promise<void> {
    return Promise.resolve();
  }
}

export function isTauri(): boolean {
  return false;
}

export type { InvokeArgs, InvokeOptions };
