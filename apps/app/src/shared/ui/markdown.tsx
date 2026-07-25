import "streamdown/styles.css";
import "katex/dist/katex.min.css";

import { memo } from "react";
import { Streamdown, type ControlsConfig, type MermaidOptions } from "streamdown";
import { cjk } from "@streamdown/cjk";
import { code } from "@streamdown/code";
import { createMathPlugin } from "@streamdown/math";
import { mermaid } from "@streamdown/mermaid";

import { cn } from "@/shared/lib/utils";
import type { Theme } from "@/shared/lib/theme";
import { useTheme } from "@/shared/store";

// Streamdown's plugins are what a komo reply actually needs, and each earns its
// weight: shiki highlighting for the shell/rust/ts the agent quotes, KaTeX for
// math, mermaid for the diagrams it likes to draw, and CJK-aware emphasis —
// plain GFM mis-parses `**加粗**` against an adjacent CJK character, which in a
// mostly-Chinese UI is the common case, not an edge one.
//
// `singleDollarTextMath` is off in the plugin's defaults because it can catch
// `$5 … $10` in prose, but a model asked for math writes `$x$` far more often
// than komo quotes two dollar amounts in one line — and raw LaTeX on screen is
// the worse failure. Hoisted: the object is part of Streamdown's memo key.
const PLUGINS = {
  code,
  math: createMathPlugin({ singleDollarTextMath: true }),
  mermaid,
  cjk,
};

// Mermaid picks its colors when it initializes, so unlike the rest of the tree
// it can't ride Tailwind's `dark:` variant — it takes the app theme as config.
// One frozen object per theme, again for the memo key.
const MERMAID: Record<Theme, MermaidOptions> = {
  light: { config: { theme: "neutral" } },
  dark: { config: { theme: "dark" } },
};

// Pan/zoom stacks buttons on top of the diagram, which inside a chat bubble
// covers the thing it is meant to help read; fullscreen already answers "show me
// it bigger". Every other control (code/table copy + download) stays on.
const CONTROLS: ControlsConfig = { mermaid: { panZoom: false } };

/** An assistant reply, rendered as markdown.
 *
 *  `mode="static"`: a komo reply arrives whole — the gateway's stream carries
 *  tool frames, not token deltas — so there is no half-written fence to repair
 *  and nothing to animate. */
export const Markdown = memo(function Markdown({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  const theme = useTheme();
  return (
    <Streamdown
      mode="static"
      plugins={PLUGINS}
      mermaid={MERMAID[theme]}
      controls={CONTROLS}
      className={cn("space-y-3", className)}
    >
      {text}
    </Streamdown>
  );
});
