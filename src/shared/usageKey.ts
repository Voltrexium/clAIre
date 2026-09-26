/** Same id as `usage_key_id` in the Rust backend. Not a reversible encoding of the secret. */

const cache = new Map<string, string>();

function plainId(key: string): string | null {
  const trimmed = key.trim();
  if (!trimmed || trimmed === "local" || trimmed.startsWith("sha256:")) return trimmed;
  return null;
}

/** Synchronous lookup. Empty until `hashUsageKey` has resolved for that secret. */
export function usageKeyId(key: string): string {
  const plain = plainId(key);
  if (plain !== null) return plain;
  return cache.get(key.trim()) ?? "";
}

export async function hashUsageKey(key: string): Promise<string> {
  const plain = plainId(key);
  if (plain !== null) return plain;
  const trimmed = key.trim();
  const hit = cache.get(trimmed);
  if (hit) return hit;
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(trimmed));
  const hex = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  const id = `sha256:${hex}`;
  cache.set(trimmed, id);
  return id;
}
