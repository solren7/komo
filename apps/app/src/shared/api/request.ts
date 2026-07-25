// Typed helpers over the installed client. Every failure becomes an `ApiError`
// so react-query surfaces it — nothing here degrades to an empty value.

import { ApiError } from "./errors";
import { getClient } from "./runtime";

export async function apiGet<T>(path: string): Promise<T> {
  const res = await getClient().api<T>({ path, method: "GET" });
  if (!res.ok) throw new ApiError(res.error || `HTTP ${res.status}`, res.status);
  return res.data as T;
}

/** GET a `{ "<key>": T }` envelope and return the inner value.
 *  A missing key is a *contract violation* (the gateway renamed a field), not
 *  an empty list — so it throws rather than silently rendering nothing. */
export async function apiField<T>(path: string, key: string): Promise<T> {
  const envelope = await apiGet<Record<string, unknown>>(path);
  const value = envelope?.[key];
  if (value === undefined) {
    throw new ApiError(`响应缺少字段 "${key}"（${path}）`, 200);
  }
  return value as T;
}

export async function apiPost<T = unknown>(path: string, body?: unknown): Promise<T> {
  const res = await getClient().api<T>({ path, method: "POST", body });
  if (!res.ok) throw new ApiError(res.error || `HTTP ${res.status}`, res.status);
  return res.data as T;
}
