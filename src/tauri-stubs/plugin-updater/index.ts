// Stub for @tauri-apps/plugin-updater on web
export interface Update {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
  available: boolean;
  downloadedBytes?: unknown;
}

export interface UpdateOptions {
  pubkey?: string;
  headers?: Record<string, string>;
}

export async function check(_options?: UpdateOptions): Promise<Update | null> {
  return null;
}
