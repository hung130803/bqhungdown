import i18n from "@/i18n";

/**
 * Convert any error thrown by Tauri `invoke()` into a human-readable string.
 *
 * Backend `AppError` is serialized as `{ kind, data? }`. We map `kind` to an
 * i18n key under `errors.*` so the UI message stays localized.
 *
 * Falls back to `error.message`, then `JSON.stringify`, then "errors.Generic".
 */
export function formatError(err: unknown): string {
  if (err == null) return "";
  if (typeof err === "string") return err;

  const obj = err as Record<string, unknown>;

  // AppError tagged union: { kind: "InvalidUrl", data?: ... }
  const kind = typeof obj.kind === "string" ? obj.kind : null;
  if (kind) {
    const key = `errors.${kind}`;
    const translated = i18n.t(key);
    // i18n.t returns the key itself if missing; only use it when truly resolved.
    const isResolved = translated !== key && typeof translated === "string";
    const base = isResolved ? translated : (i18n.t("errors.Generic") as string);
    const data = obj.data;
    if (data && (typeof data === "string" || typeof data === "number")) {
      return `${base}: ${data}`;
    }
    return base;
  }

  if (typeof obj.message === "string") return obj.message;

  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}
