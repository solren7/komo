import { apiGet } from "@/shared/api/request";
import type { ModelMenu } from "@/shared/types";

/** What a session may be switched to. The whole body is the value (not a
 *  single-field envelope), so this reads it directly rather than via `apiField`. */
export function fetchModelMenu(): Promise<ModelMenu> {
  return apiGet<ModelMenu>("/api/models");
}
