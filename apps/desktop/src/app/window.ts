// The Mesh window: one function from what is known to what is on the screen.
//
// # Why a text frame and not a React tree
//
// There is no window in this repository yet, and the reason is written down in `apps/desktop/
// README.md` §1: a Tauri host is a Rust crate whose manifest turns the architecture check red
// until a file outside this task's allowed paths promotes it, and a React tree needs an npm
// dependency graph that no gate here may download. So this is the window a repository with those
// two constraints can actually run: a frame drawn in text, on a terminal, showing what a REAL
// background service answered. It is screenshot-able, a person can start it, and every word in it
// went through the same copy and the same vocabulary lint a React tree would use. When the Tauri
// shell lands it binds to `WindowView` — the shape below — and this renderer is deleted, not
// ported.
//
// # Why the renderer is pure
//
// `renderWindow` takes a view and returns a string. No socket, no clock, no terminal. That is what
// lets `window.test.ts` assert the exact frame for a service that is running, one that is not
// running, and one that is too old to talk to — three situations that are otherwise reachable only
// by timing. `live.ts` owns everything impure.
//
// # The one presentation decision in here
//
// The client has one internal state for "retrying", and a person meets it in two completely
// different situations: the service died under a window that was working, or the service was never
// started. `connectionSentence` is where those two part company, from `everConnected`. Everything
// else is layout.

import type { ConnectionState } from '../ipc/client.ts';
import { CONNECTION_COPY } from '../strings/connection.ts';
import { WINDOW_COPY, durationWording, versionWording } from '../strings/window.ts';
import { UI_OPERATIONS, copyFor } from './operations.ts';
import type { ServiceFacts } from './facts.ts';

/** Everything the window draws from. */
export type WindowView = {
  /** The endpoint this window is watching. */
  readonly endpoint: string;
  /** What the connection is doing. */
  readonly connection: ConnectionState;
  /** Whether this window has ever been connected on this endpoint. */
  readonly everConnected: boolean;
  /** What the service last said about itself, or `null` before it has said anything. */
  readonly facts: ServiceFacts | null;
  /** A sentence about something that went wrong, or `null`. */
  readonly fault: string | null;
  /** Whether this window stays open, which changes only the footer. */
  readonly staysOpen: boolean;
};

/** How wide the frame is inside its border. */
export const INNER_WIDTH = 76;

/** How wide a field label is, so every value in a section starts in the same column. */
const LABEL_WIDTH = 20;

const HORIZONTAL = '─'.repeat(INNER_WIDTH + 2);
const TOP = `┌${HORIZONTAL}┐`;
const MIDDLE = `├${HORIZONTAL}┤`;
const BOTTOM = `└${HORIZONTAL}┘`;

/** Break `text` into lines no wider than `width`, splitting a word only when it cannot fit. */
export const wrapText = (text: string, width: number): string[] => {
  const words = text.split(/\s+/).filter((word) => word.length > 0);
  const lines: string[] = [];
  let current = '';
  const flush = (): void => {
    if (current.length > 0) {
      lines.push(current);
      current = '';
    }
  };
  for (const word of words) {
    if (word.length > width) {
      flush();
      for (let at = 0; at < word.length; at += width) lines.push(word.slice(at, at + width));
      continue;
    }
    const candidate = current.length === 0 ? word : `${current} ${word}`;
    if (candidate.length > width) {
      flush();
      current = word;
    } else {
      current = candidate;
    }
  }
  flush();
  return lines.length === 0 ? [''] : lines;
};

/** One indented paragraph. */
const paragraph = (text: string): string[] => wrapText(text, INNER_WIDTH - 2).map((line) => `  ${line}`);

/**
 * One labelled value, wrapped under a hanging indent so the column survives a long value.
 *
 * A label too long for the column takes a line of its own rather than running into its value.
 * Copy is edited by people and the day somebody writes a longer label is the day this would
 * otherwise print `What this version offersready`, which is how a window loses somebody's trust.
 */
const field = (label: string, value: string): string[] => {
  const head = `  ${label.padEnd(LABEL_WIDTH)}`;
  const hang = ' '.repeat(head.length);
  const wrapped = wrapText(value, INNER_WIDTH - head.length);
  if (label.length > LABEL_WIDTH - 2) return [`  ${label}`, ...wrapped.map((line) => `${hang}${line}`)];
  return wrapped.map((line, index) => (index === 0 ? `${head}${line}` : `${hang}${line}`));
};

/** The sentence for the link, which is the one place two situations share an internal state. */
export const connectionSentence = (view: WindowView): string => {
  if (view.connection === 'reconnecting' && !view.everConnected) return CONNECTION_COPY.notRunning;
  return CONNECTION_COPY[view.connection];
};

/** What the service section says. */
const serviceLines = (facts: ServiceFacts | null): string[] => {
  if (facts === null) return paragraph(WINDOW_COPY.nothingYet);
  return [
    ...field(WINDOW_COPY.servingLabel, facts.serving ? WINDOW_COPY.serving : WINDOW_COPY.notServing),
    ...field(WINDOW_COPY.interfaceLabel, versionWording(facts.surfaceVersion)),
    ...field(WINDOW_COPY.startupLabel, facts.startupSentence),
    ...field(WINDOW_COPY.startupTookLabel, durationWording(facts.startupElapsedMs)),
  ];
};

/** What the operations section says: this window's own catalogue, against what the service has. */
const operationLines = (facts: ServiceFacts | null): string[] =>
  UI_OPERATIONS.flatMap((operation) => {
    const copy = copyFor(operation.id);
    const label = copy?.label ?? operation.id;
    if (facts === null) return field(label, copy?.description ?? '');
    const available = facts.methodNames.includes(operation.method) && operation.version <= facts.surfaceVersion
      ? WINDOW_COPY.operationAvailable
      : WINDOW_COPY.operationMissing;
    return field(label, available);
  });

/** Draw the window. */
export const renderWindow = (view: WindowView): string => {
  const sections: string[][] = [
    [WINDOW_COPY.title],
    [WINDOW_COPY.connectionHeading, ...paragraph(connectionSentence(view)), ...field(WINDOW_COPY.endpointLabel, view.endpoint)],
    [WINDOW_COPY.serviceHeading, ...serviceLines(view.facts)],
    [WINDOW_COPY.operationsHeading, ...operationLines(view.facts)],
  ];
  if (view.fault !== null) sections.push(paragraph(view.fault));
  sections.push([view.staysOpen ? WINDOW_COPY.quitHint : WINDOW_COPY.singleFrameHint]);

  const body = sections
    .map((section) => section.map((line) => `│ ${line.padEnd(INNER_WIDTH)} │`))
    .reduce<string[]>((all, section, index) => (index === 0 ? section : [...all, MIDDLE, ...section]), []);
  return [TOP, ...body, BOTTOM, ''].join('\n');
};
