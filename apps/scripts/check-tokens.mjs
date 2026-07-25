// Guards the theme boundary: component code may only reference semantic tokens
// (bg-primary, text-muted-foreground, …), never a palette variable or a raw
// color literal. The one sanctioned exception is the status ramp in
// shared/ui/badge.tsx and the handful of status colors documented in
// apps/app/README.md.
//
// Runs as part of `bun run lint`, because a lint rule is the only thing that
// keeps a convention alive after the refactor that introduced it.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOTS = ["app/src", "desktop/src", "web/src"];
const BANNED = [
  { re: /--mc-[a-z0-9-]+/g, why: "旧的 --mc-* 调色板已删除，请用语义 token" },
  {
    re: /(?:bg|text|border|ring|shadow|fill|stroke)-\[#[0-9a-fA-F]{3,8}\]/g,
    why: "不要在类名里写死颜色",
  },
];

function* files(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* files(path);
    else if (/\.(tsx?|css)$/.test(entry)) yield path;
  }
}

let failures = 0;
for (const root of ROOTS) {
  for (const file of files(root)) {
    const text = readFileSync(file, "utf8");
    for (const { re, why } of BANNED) {
      for (const match of text.matchAll(re)) {
        const line = text.slice(0, match.index).split("\n").length;
        console.error(`${file}:${line}  ${match[0]}  — ${why}`);
        failures++;
      }
    }
  }
}

if (failures > 0) {
  console.error(`\n${failures} 处主题违规。`);
  process.exit(1);
}
console.log("theme tokens ok");
