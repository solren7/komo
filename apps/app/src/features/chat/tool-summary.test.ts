import { describe, expect, it } from "vitest";

import { summarizeToolRound, toolTitle } from "./tool-summary";

describe("summarizeToolRound", () => {
  it("counts calls and lists distinct names in call order", () => {
    const summary = summarizeToolRound([{ name: "shell" }, { name: "time" }, { name: "shell" }]);
    expect(summary).toEqual({ count: 3, failed: 0, names: "shell · time" });
  });

  it("elides names past the cap", () => {
    const summary = summarizeToolRound(
      [{ name: "a" }, { name: "b" }, { name: "c" }, { name: "d" }, { name: "e" }],
      3,
    );
    expect(summary.names).toBe("a · b · c +2");
  });

  it("counts failures", () => {
    const summary = summarizeToolRound([
      { name: "shell", failed: true },
      { name: "shell" },
      { name: "web_fetch", failed: true },
    ]);
    expect(summary.count).toBe(3);
    expect(summary.failed).toBe(2);
  });

  it("summarizes an empty round to nothing", () => {
    expect(summarizeToolRound([])).toEqual({ count: 0, failed: 0, names: "" });
  });
});

describe("toolTitle", () => {
  it("names the skill a skill call loaded", () => {
    expect(toolTitle("skill", { action: "view", name: "calendar" })).toBe("Skill · calendar");
  });

  it("falls back to the tool name", () => {
    expect(toolTitle("skill", { action: "list" })).toBe("skill");
    expect(toolTitle("shell", { command: "ls" })).toBe("shell");
    expect(toolTitle("time")).toBe("time");
  });
});
