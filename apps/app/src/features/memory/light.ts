// How much light a memory stands in.
//
// The encoding is not decorative and it is not "recall count → brightness". It
// is the memory system's own ladder: how close a memory sits to the model's
// prompt right now.
//
//   常驻 pinned    every turn carries it (L1)          — full sun
//   受光 active     eligible for recall (L2/L3)         — lit
//   新芽 candidate  extracted, waiting for promotion    — understory
//   荫影 archived   out of the prompt, kept on disk     — shade
//   落叶 rejected   turned down                         — fallen
//
// Recall usage modulates *within* a tier rather than replacing it, because a
// memory that has never been recalled is still in the prompt if it is pinned.
// On a fresh install every recall counter is zero, so tier is the signal that
// actually carries information; recall becomes the finer grain as it accrues.

import type { Memory } from "@/shared/types";

export type Tier = "pinned" | "active" | "candidate" | "archived" | "rejected";

export interface TierStyle {
  /** Short Chinese name, used as the row's standing label. */
  label: string;
  /** One line explaining what the tier means for the model's prompt. */
  meaning: string;
  /** Ring + ground for the row, tuned so each tier reads at a glance. */
  row: string;
  /** The light mote that carries the tier, drawn in `MemoryLight`. */
  mote: string;
}

export const TIER_ORDER: Tier[] = ["pinned", "active", "candidate", "archived", "rejected"];

export const TIERS: Record<Tier, TierStyle> = {
  pinned: {
    label: "常驻",
    meaning: "每一轮对话都会带上",
    row: "border-warning/45 bg-warning/8",
    mote: "bg-warning shadow-[0_0_10px_2px_color-mix(in_oklch,var(--warning)_55%,transparent)]",
  },
  active: {
    label: "受光",
    meaning: "可以被回忆检索到",
    row: "border-success/35 bg-success/6",
    mote: "bg-success shadow-[0_0_7px_1px_color-mix(in_oklch,var(--success)_40%,transparent)]",
  },
  candidate: {
    label: "新芽",
    meaning: "刚抽取出来，等待你确认",
    row: "border-border bg-card",
    mote: "bg-muted-foreground/55",
  },
  archived: {
    label: "荫影",
    meaning: "不再进入 prompt，仍留在库里",
    row: "border-border/50 bg-transparent",
    mote: "bg-muted-foreground/25",
  },
  rejected: {
    label: "落叶",
    meaning: "已被否决",
    row: "border-border/40 bg-transparent",
    mote: "bg-muted-foreground/20",
  },
};

export function tierOf(memory: Memory): Tier {
  if (memory.pinned) return "pinned";
  const status = memory.status.toLowerCase();
  if (status === "active") return "active";
  if (status === "candidate") return "candidate";
  if (status === "rejected") return "rejected";
  return "archived";
}

/** Chinese labels for every `MemoryKind` variant (komo-core's domain::memory). */
const KINDS: Record<string, string> = {
  profile: "画像",
  preference: "偏好",
  feedback: "反馈",
  project: "项目",
  person: "人物",
  fact: "事实",
  decision: "决定",
  reference: "线索",
};

export function kindLabel(kind: string): string {
  return KINDS[kind.toLowerCase()] ?? kind;
}

/** How komo came by this memory — every `MemoryConfidence` variant. Worth
 *  showing plainly: a guess the model made and something you wrote yourself
 *  deserve different trust when you are deciding whether to keep it. */
const CONFIDENCE: Record<string, string> = {
  extracted: "对话中抽取",
  inferred: "模型推断",
  confirmed: "已确认",
  user_written: "你亲手写的",
};

export function confidenceLabel(confidence: string): string {
  return CONFIDENCE[confidence.toLowerCase()] ?? confidence;
}

/** Actions that make sense for a memory in this tier, in the order shown.
 *
 *  Offering every verb on every row is what the old panel did — a `reject`
 *  button on an already-rejected memory is noise the operator has to read past. */
export function actionsFor(tier: Tier): ("promote" | "pin" | "reject")[] {
  switch (tier) {
    case "pinned":
      return ["reject"];
    case "active":
      return ["pin", "reject"];
    case "candidate":
      return ["promote", "pin", "reject"];
    default:
      return ["promote"];
  }
}

export const ACTION_LABELS: Record<"promote" | "pin" | "reject", string> = {
  promote: "转为受光",
  pin: "设为常驻",
  reject: "否决",
};
