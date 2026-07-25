import { describe, expect, it } from "vitest";

import { fmtTs } from "./format";

describe("fmtTs", () => {
  it("renders UTC MM-DD HH:MM with zero padding", () => {
    expect(fmtTs(Date.UTC(2026, 6, 5, 3, 7) / 1000)).toBe("07-05 03:07");
  });

  it("handles the epoch", () => {
    expect(fmtTs(0)).toBe("01-01 00:00");
  });
});
