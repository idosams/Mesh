import { Button } from "./button";
import type { KeyboardEvent } from "react";
import { rovingSelectionIndex } from "../models/roving-selection";

type SegmentedControlProps = {
  label: string;
  options: readonly string[];
  value: string;
  onChange: (value: string) => void;
  proof?: string;
};

export function SegmentedControl({ label, options, value, onChange, proof }: SegmentedControlProps) {
  const moveSelection = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = rovingSelectionIndex(options.length, index, event.key, "all-wrap");
    if (next === null) return;
    event.preventDefault();
    const buttons = event.currentTarget.parentElement
      ?.querySelectorAll<HTMLButtonElement>("[data-segment-option]");
    buttons?.[next]?.focus();
    onChange(options[next]);
  };
  return (
    <div className="inline-flex rounded-lg border border-border bg-background p-1" role="group" aria-label={label} data-mesh-proof={proof}>
      {options.map((option) => (
        <Button
          key={option}
          data-segment-option
          size="compact"
          variant={option === value ? "primary" : "quiet"}
          aria-pressed={option === value}
          tabIndex={option === value ? 0 : -1}
          onClick={() => onChange(option)}
          onKeyDown={(event) => moveSelection(event, options.indexOf(option))}
        >
          {option}
        </Button>
      ))}
    </div>
  );
}
