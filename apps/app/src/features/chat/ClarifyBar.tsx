import { useState } from "react";

import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

/** The agent asked a question mid-turn (the `ask_user` sentinel tool); the turn
 *  is suspended until this answer posts. */
export function ClarifyBar({
  question,
  onAnswer,
}: {
  question: string;
  onAnswer: (text: string) => void;
}) {
  const [text, setText] = useState("");
  const submit = () => {
    const trimmed = text.trim();
    if (trimmed) onAnswer(trimmed);
  };
  return (
    <div className="mx-4 mb-2 rounded-xl border border-primary/30 bg-primary/5 px-3.5 py-2.5">
      <div className="mb-1.5 font-semibold text-foreground">❓ {question}</div>
      <div className="flex gap-2">
        <Input
          className="flex-1"
          value={text}
          placeholder="输入你的回答…"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
        />
        <Button size="sm" onClick={submit}>
          回答
        </Button>
      </div>
    </div>
  );
}
