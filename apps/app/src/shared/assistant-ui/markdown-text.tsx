import "@assistant-ui/react-markdown/styles/dot.css";

import {
  type CodeHeaderProps,
  MarkdownTextPrimitive,
  type SyntaxHighlighterProps,
  unstable_memoizeMarkdownComponents as memoizeMarkdownComponents,
  useIsMarkdownCodeBlock,
} from "@assistant-ui/react-markdown";
import { makePrismSyntaxHighlighter } from "@assistant-ui/react-syntax-highlighter/full";
import { CheckIcon, CopyIcon } from "lucide-react";
import { oneDark, oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";
import remarkGfm from "remark-gfm";
import { type FC, memo, useState } from "react";

import { cn } from "@/shared/lib/utils";
import { useTheme } from "@/shared/store";

const MarkdownTextImpl = () => {
  return (
    <MarkdownTextPrimitive
      remarkPlugins={[remarkGfm]}
      className="aui-md"
      components={defaultComponents}
    />
  );
};

export const MarkdownText = memo(MarkdownTextImpl);

// Prism-based highlighting. The token colors come from the prism theme; the
// background/padding is stripped so our own `pre` shows through.
const HL_CONFIG = {
  customStyle: { margin: 0, padding: 0, background: "transparent", fontSize: "0.875rem" },
  codeTagProps: { style: { background: "transparent", fontFamily: "inherit" } },
} as const;
const DarkHighlighter = makePrismSyntaxHighlighter({ style: oneDark, ...HL_CONFIG });
const LightHighlighter = makePrismSyntaxHighlighter({ style: oneLight, ...HL_CONFIG });

// The async Prism loader resolves by canonical language name, not alias — map
// the short forms the model tends to emit so ```ts / ```py still highlight.
const LANG_ALIAS: Record<string, string> = {
  ts: "typescript",
  js: "javascript",
  py: "python",
  rb: "ruby",
  rs: "rust",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  yml: "yaml",
  md: "markdown",
  cs: "csharp",
  golang: "go",
  "c++": "cpp",
};

const SyntaxHighlighter: FC<SyntaxHighlighterProps> = ({ language, ...props }) => {
  const theme = useTheme();
  const Highlighter = theme === "dark" ? DarkHighlighter : LightHighlighter;
  return <Highlighter language={LANG_ALIAS[language] ?? language} {...props} />;
};

const CodeHeader: FC<CodeHeaderProps> = ({ language, code }) => {
  const { isCopied, copyToClipboard } = useCopyToClipboard();
  const onCopy = () => {
    if (!code || isCopied) return;
    copyToClipboard(code);
  };
  return (
    <div className="mt-3 flex items-center justify-between rounded-t-lg border border-b-0 border-border bg-muted px-3.5 py-1.5 text-xs">
      <span className="font-medium lowercase text-muted-foreground">{language}</span>
      <button
        type="button"
        onClick={onCopy}
        title="复制"
        className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        {isCopied ? <CheckIcon className="size-3.5" /> : <CopyIcon className="size-3.5" />}
      </button>
    </div>
  );
};

function useCopyToClipboard({ copiedDuration = 3000 }: { copiedDuration?: number } = {}) {
  const [isCopied, setIsCopied] = useState(false);
  const copyToClipboard = (value: string) => {
    if (!value || typeof navigator === "undefined" || !navigator.clipboard) return;
    void navigator.clipboard.writeText(value).then(
      () => {
        setIsCopied(true);
        setTimeout(() => setIsCopied(false), copiedDuration);
      },
      () => {},
    );
  };
  return { isCopied, copyToClipboard };
}

const defaultComponents = memoizeMarkdownComponents({
  SyntaxHighlighter,
  CodeHeader,
  h1: ({ className, ...props }) => (
    <h1
      className={cn("mt-5 mb-2 text-xl font-semibold first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  h2: ({ className, ...props }) => (
    <h2
      className={cn("mt-5 mb-2 text-lg font-semibold first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  h3: ({ className, ...props }) => (
    <h3
      className={cn("mt-4 mb-1.5 text-base font-semibold first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  h4: ({ className, ...props }) => (
    <h4
      className={cn("mt-3.5 mb-1 text-base font-medium first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  h5: ({ className, ...props }) => (
    <h5
      className={cn("mt-3 mb-1 text-sm font-semibold first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  h6: ({ className, ...props }) => (
    <h6
      className={cn("mt-3 mb-1 text-sm font-medium first:mt-0 last:mb-0", className)}
      {...props}
    />
  ),
  p: ({ className, ...props }) => (
    <p className={cn("my-2.5 leading-relaxed first:mt-0 last:mb-0", className)} {...props} />
  ),
  a: ({ className, ...props }) => (
    <a
      className={cn("text-primary underline underline-offset-2 hover:text-primary/80", className)}
      {...props}
    />
  ),
  blockquote: ({ className, ...props }) => (
    <blockquote
      className={cn("my-3 border-s-2 border-input ps-4 text-muted-foreground", className)}
      {...props}
    />
  ),
  ul: ({ className, ...props }) => (
    <ul className={cn("my-3 ms-5 list-disc [&>li]:mt-1", className)} {...props} />
  ),
  ol: ({ className, ...props }) => (
    <ol className={cn("my-3 ms-5 list-decimal [&>li]:mt-1", className)} {...props} />
  ),
  hr: ({ className, ...props }) => (
    <hr className={cn("my-3 border-border", className)} {...props} />
  ),
  table: ({ className, ...props }) => (
    <table
      className={cn(
        "my-3 w-full overflow-x-auto rounded-lg border border-separate border-spacing-0 border-border",
        className,
      )}
      {...props}
    />
  ),
  th: ({ className, ...props }) => (
    <th
      className={cn(
        "border-b border-border bg-muted px-3 py-1.5 text-start font-medium",
        className,
      )}
      {...props}
    />
  ),
  td: ({ className, ...props }) => (
    <td
      className={cn("border-b border-border px-3 py-1.5 [tr:last-child_&]:border-b-0", className)}
      {...props}
    />
  ),
  tr: ({ className, ...props }) => <tr className={cn(className)} {...props} />,
  sup: ({ className, ...props }) => (
    <sup className={cn("[&>a]:text-xs [&>a]:no-underline", className)} {...props} />
  ),
  pre: ({ className, ...props }) => (
    <pre
      className={cn(
        "overflow-x-auto rounded-b-lg border border-t-0 border-border bg-muted/40 p-3.5 font-mono text-sm last:mb-0",
        className,
      )}
      {...props}
    />
  ),
  code: function Code({ className, ...props }) {
    const isCodeBlock = useIsMarkdownCodeBlock();
    return (
      <code
        className={cn(
          "font-mono",
          !isCodeBlock && "rounded bg-muted px-1.5 py-0.5 text-[0.9em]",
          className,
        )}
        {...props}
      />
    );
  },
});
