// What a collapsed round of tool calls says on its one line.
//
// A turn can spend a dozen calls before it answers, and none of them is the
// answer — so the display gives the whole round one line and lets it expand.
// That line has to carry the useful part: which tools ran, how many times, and
// whether anything failed. This module is that reduction, kept pure so it is
// the part under test.

/** One call as the collapsed line needs it — the rest lives in the expansion. */
export interface ToolRoundCall {
  name: string;
  failed?: boolean;
}

/** How many distinct tool names the line spells out before eliding the rest. */
export const TOOL_NAME_CAP = 3;

export interface ToolRoundSummary {
  /** Calls in the round (not distinct names — three `shell` calls count three). */
  count: number;
  /** How many of them failed. */
  failed: number;
  /** Distinct tool names in call order, capped with a `+n` tail. */
  names: string;
}

/** Reduce a round of calls to its one-line summary. Names are deduped because a
 *  loop that calls `shell` five times should read `shell`, not `shell` five
 *  times — the count already says how often. */
export function summarizeToolRound(
  calls: readonly ToolRoundCall[],
  cap = TOOL_NAME_CAP,
): ToolRoundSummary {
  const distinct: string[] = [];
  for (const call of calls) if (!distinct.includes(call.name)) distinct.push(call.name);
  const shown = distinct.slice(0, cap);
  const elided = distinct.length - shown.length;
  return {
    count: calls.length,
    failed: calls.filter((call) => call.failed).length,
    names: elided > 0 ? `${shown.join(" · ")} +${elided}` : shown.join(" · "),
  };
}

/** The label for a single call. `skill` is the one tool whose name says nothing
 *  on its own — which skill it loaded is the interesting part. */
export function toolTitle(toolName: string, args?: Record<string, unknown>): string {
  const skill = typeof args?.name === "string" ? args.name : null;
  return toolName === "skill" && skill ? `Skill · ${skill}` : toolName;
}
