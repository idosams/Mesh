import type { ReactNode } from "react";

export function WorkspaceEntryShell({ children }: Readonly<{ children: ReactNode }>) {
  return <div className="mx-auto w-full max-w-7xl px-1 sm:px-2 lg:px-4">{children}</div>;
}
