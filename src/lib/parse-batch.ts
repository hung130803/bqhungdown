import { isValidUrl } from "./url";

export function parseBatch(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map(s => s.trim())
    .filter(s => s.length > 0 && isValidUrl(s));
}
