import { describe, expect, it } from "vitest";

import { headerFor, newSessionId } from "./session-id";

describe("newSessionId", () => {
  it("tags the host and keeps the api: prefix", () => {
    expect(newSessionId("desktop")).toMatch(/^api:gui-desktop-[0-9a-f-]{36}$/);
    expect(newSessionId("web")).toMatch(/^api:gui-web-[0-9a-f-]{36}$/);
  });

  it("is unique per call", () => {
    expect(newSessionId("web")).not.toBe(newSessionId("web"));
  });
});

describe("headerFor", () => {
  it("strips the api: prefix the server re-adds", () => {
    expect(headerFor("api:gui-web-abc")).toBe("gui-web-abc");
  });

  it("passes through an id that has no prefix", () => {
    expect(headerFor("feishu:oc_1")).toBe("feishu:oc_1");
  });
});
