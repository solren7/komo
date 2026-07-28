import { describe, expect, it } from "vitest";

import type { ModelMenu, ModelOption } from "@/shared/types";
import { byProvider, selectedContextWindow, selectedOption } from "./ModelPicker";

function option(id: string, provider: string, efforts: string[] = []): ModelOption {
  return {
    id,
    provider,
    model: id.includes(":") ? id.slice(id.indexOf(":") + 1) : id,
    context_window: provider === "deepseek" ? 128_000 : 400_000,
    efforts,
  };
}

const LEVELS = ["low", "medium", "high"];

const menu: ModelMenu = {
  provider: "codex",
  default_model: "gpt-5.6-terra",
  models: [
    option("gpt-5.6-terra", "codex", LEVELS),
    option("gpt-5.4-mini", "codex", LEVELS),
    option("deepseek:deepseek-chat", "deepseek"),
  ],
};

describe("selectedOption", () => {
  it("falls back to the gateway default when the session has no choice", () => {
    expect(selectedOption(menu, "")?.id).toBe("gpt-5.6-terra");
  });

  it("resolves an explicit choice, including a cross-provider one", () => {
    expect(selectedOption(menu, "deepseek:deepseek-chat")?.provider).toBe("deepseek");
    expect(selectedOption(menu, "gpt-5.4-mini")?.model).toBe("gpt-5.4-mini");
  });

  it("is undefined for an id no longer on the menu, and with no menu at all", () => {
    // A session can store a model the config has since dropped.
    expect(selectedOption(menu, "gpt-4.1")).toBeUndefined();
    expect(selectedOption(undefined, "gpt-5.5")).toBeUndefined();
  });
});

describe("selectedContextWindow", () => {
  it("tracks the selected model, not the gateway default", () => {
    expect(selectedContextWindow(menu, "")).toBe(400_000);
    expect(selectedContextWindow(menu, "deepseek:deepseek-chat")).toBe(128_000);
  });

  it("is null when the capacity is unknown", () => {
    expect(selectedContextWindow(menu, "gpt-4.1")).toBeNull();
    expect(selectedContextWindow(undefined, "")).toBeNull();
  });
});

describe("byProvider", () => {
  it("groups in menu order, keeping each provider's models together", () => {
    expect(
      byProvider(menu).map(([provider, options]) => [provider, options.map((o) => o.id)]),
    ).toEqual([
      ["codex", ["gpt-5.6-terra", "gpt-5.4-mini"]],
      ["deepseek", ["deepseek:deepseek-chat"]],
    ]);
  });

  it("returns a single group for a single-provider menu", () => {
    const single: ModelMenu = { ...menu, models: menu.models.slice(0, 2) };
    expect(byProvider(single)).toHaveLength(1);
  });

  it("returns nothing before the menu has loaded", () => {
    expect(byProvider(undefined)).toEqual([]);
  });
});
