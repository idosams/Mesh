import type { WorkspaceEntryChoice, WorkspaceNativeChange, WorkspaceWorkChoice } from "./workspace-files-changes";
import {
  BOUNDED_CHOICE_LIMIT,
  boundedChoiceProjection,
  type BoundedChoiceProjection,
} from "./choice-projection";

export const WORKSPACE_CHOICE_ROW_LIMIT = BOUNDED_CHOICE_LIMIT;
export const WORKSPACE_FILE_PREVIEW_LINE_LIMIT = 500;
export const WORKSPACE_FILE_PREVIEW_CHARACTER_LIMIT = 131_072;

export type WorkspaceFilePresentation = Readonly<{
  category: "code" | "document" | "image" | "config" | "data" | "system" | "file";
  label: string;
  shortLabel: string;
}>;

export type WorkspaceTextPreview = Readonly<{
  lines: readonly string[];
  totalLines: number;
  truncated: boolean;
}>;

const FILE_PRESENTATIONS: Readonly<Record<string, WorkspaceFilePresentation>> = Object.freeze({
  c: Object.freeze({ category: "code", label: "C source", shortLabel: "C" }),
  cc: Object.freeze({ category: "code", label: "C++ source", shortLabel: "C++" }),
  cpp: Object.freeze({ category: "code", label: "C++ source", shortLabel: "C++" }),
  css: Object.freeze({ category: "code", label: "CSS stylesheet", shortLabel: "CSS" }),
  go: Object.freeze({ category: "code", label: "Go source", shortLabel: "GO" }),
  h: Object.freeze({ category: "code", label: "C header", shortLabel: "H" }),
  hpp: Object.freeze({ category: "code", label: "C++ header", shortLabel: "HPP" }),
  html: Object.freeze({ category: "code", label: "HTML document", shortLabel: "HTML" }),
  java: Object.freeze({ category: "code", label: "Java source", shortLabel: "JAVA" }),
  js: Object.freeze({ category: "code", label: "JavaScript", shortLabel: "JS" }),
  jsx: Object.freeze({ category: "code", label: "JavaScript JSX", shortLabel: "JSX" }),
  py: Object.freeze({ category: "code", label: "Python source", shortLabel: "PY" }),
  rb: Object.freeze({ category: "code", label: "Ruby source", shortLabel: "RB" }),
  rs: Object.freeze({ category: "code", label: "Rust source", shortLabel: "RS" }),
  sh: Object.freeze({ category: "code", label: "Shell script", shortLabel: "SH" }),
  sql: Object.freeze({ category: "code", label: "SQL source", shortLabel: "SQL" }),
  swift: Object.freeze({ category: "code", label: "Swift source", shortLabel: "SWIFT" }),
  ts: Object.freeze({ category: "code", label: "TypeScript", shortLabel: "TS" }),
  tsx: Object.freeze({ category: "code", label: "TypeScript TSX", shortLabel: "TSX" }),
  md: Object.freeze({ category: "document", label: "Markdown", shortLabel: "MD" }),
  txt: Object.freeze({ category: "document", label: "Plain text", shortLabel: "TXT" }),
  pdf: Object.freeze({ category: "document", label: "PDF document", shortLabel: "PDF" }),
  doc: Object.freeze({ category: "document", label: "Word document", shortLabel: "DOC" }),
  docx: Object.freeze({ category: "document", label: "Word document", shortLabel: "DOCX" }),
  ppt: Object.freeze({ category: "document", label: "PowerPoint presentation", shortLabel: "PPT" }),
  pptx: Object.freeze({ category: "document", label: "PowerPoint presentation", shortLabel: "PPTX" }),
  xls: Object.freeze({ category: "document", label: "Excel workbook", shortLabel: "XLS" }),
  xlsx: Object.freeze({ category: "document", label: "Excel workbook", shortLabel: "XLSX" }),
  gif: Object.freeze({ category: "image", label: "GIF image", shortLabel: "GIF" }),
  jpeg: Object.freeze({ category: "image", label: "JPEG image", shortLabel: "JPEG" }),
  jpg: Object.freeze({ category: "image", label: "JPEG image", shortLabel: "JPG" }),
  png: Object.freeze({ category: "image", label: "PNG image", shortLabel: "PNG" }),
  svg: Object.freeze({ category: "image", label: "SVG image", shortLabel: "SVG" }),
  webp: Object.freeze({ category: "image", label: "WebP image", shortLabel: "WEBP" }),
  env: Object.freeze({ category: "config", label: "Environment configuration", shortLabel: "ENV" }),
  csv: Object.freeze({ category: "data", label: "CSV data", shortLabel: "CSV" }),
  json: Object.freeze({ category: "data", label: "JSON data", shortLabel: "JSON" }),
  lock: Object.freeze({ category: "config", label: "Dependency lockfile", shortLabel: "LOCK" }),
  toml: Object.freeze({ category: "config", label: "TOML configuration", shortLabel: "TOML" }),
  yaml: Object.freeze({ category: "config", label: "YAML configuration", shortLabel: "YAML" }),
  yml: Object.freeze({ category: "config", label: "YAML configuration", shortLabel: "YML" }),
  xml: Object.freeze({ category: "data", label: "XML data", shortLabel: "XML" }),
});

export function workspaceFilePresentation(path: string): WorkspaceFilePresentation {
  const name = path.split("/").at(-1) ?? path;
  if (name === ".DS_Store") return Object.freeze({ category: "system", label: "macOS folder metadata", shortLabel: "SYS" });
  const boundary = name.lastIndexOf(".");
  const extension = boundary > 0 ? name.slice(boundary + 1).toLocaleLowerCase() : "";
  if (extension && FILE_PRESENTATIONS[extension]) return FILE_PRESENTATIONS[extension];
  if (extension) return Object.freeze({ category: "file", label: `${extension.toLocaleUpperCase()} file`, shortLabel: extension.slice(0, 5).toLocaleUpperCase() });
  return Object.freeze({ category: "file", label: "File", shortLabel: "FILE" });
}

export function workspaceTextPreview(
  text: string,
  maximumLines = WORKSPACE_FILE_PREVIEW_LINE_LIMIT,
  maximumCharacters = WORKSPACE_FILE_PREVIEW_CHARACTER_LIMIT,
): WorkspaceTextPreview {
  if (!Number.isSafeInteger(maximumLines) || maximumLines < 1 || !Number.isSafeInteger(maximumCharacters) || maximumCharacters < 1) {
    throw new Error("The file preview bound was invalid.");
  }
  const sourceLines = text.split("\n");
  const visible: string[] = [];
  let characters = 0;
  for (const line of sourceLines) {
    if (visible.length >= maximumLines || characters + line.length > maximumCharacters) break;
    visible.push(line);
    characters += line.length + 1;
  }
  return Object.freeze({
    lines: Object.freeze(visible),
    totalLines: sourceLines.length,
    truncated: visible.length < sourceLines.length,
  });
}

export type WorkspaceChoiceProjection = BoundedChoiceProjection<WorkspaceWorkChoice>;

export function workspaceChoiceProjection(
  choices: readonly WorkspaceWorkChoice[],
  filterText: string,
  selectedValue: string,
  maximum = WORKSPACE_CHOICE_ROW_LIMIT,
): WorkspaceChoiceProjection {
  return boundedChoiceProjection(
    choices,
    filterText,
    selectedValue,
    (choice) => choice.value,
    (choice) => [choice.value, choice.label],
    maximum,
  );
}

export type WorkspaceChangeFilter = "all" | WorkspaceNativeChange["code"];

export type WorkspaceChangeGroup = Readonly<{
  folder: string;
  changes: readonly WorkspaceNativeChange[];
}>;

export type WorkspaceChangeNavigation = Readonly<{
  previous: string | null;
  next: string | null;
}>;

export const WORKSPACE_CHANGE_ROW_LIMIT = 500;

export type WorkspaceChangeProjection = Readonly<{
  groups: readonly WorkspaceChangeGroup[];
  orderedPaths: readonly string[];
  matched: number;
  displayed: number;
  offset: number;
  truncated: boolean;
}>;

export function workspaceChangeProjection(
  changes: readonly WorkspaceNativeChange[],
  filterText: string,
  filter: WorkspaceChangeFilter,
  maximum = WORKSPACE_CHANGE_ROW_LIMIT,
  selectedPath = "",
): WorkspaceChangeProjection {
  if (!Number.isSafeInteger(maximum) || maximum < 1) throw new Error("The visible change-row bound was invalid.");
  const complete = workspaceChangeGroups(changes, filterText, filter);
  const ordered = complete.flatMap((group) => group.changes.map((change) => ({
    folder: group.folder,
    change,
  })));
  const orderedPaths = Object.freeze(ordered.map(({ change }) => change.path));
  const matched = ordered.length;
  const selectedIndex = selectedPath ? orderedPaths.indexOf(selectedPath) : -1;
  const pageOffset = selectedIndex >= maximum ? Math.floor(selectedIndex / maximum) * maximum : 0;
  const offset = Math.min(pageOffset, Math.max(0, matched - maximum));
  const windowGroups: { folder: string; changes: WorkspaceNativeChange[] }[] = [];
  for (const { folder, change } of ordered.slice(offset, offset + maximum)) {
    const previous = windowGroups.at(-1);
    if (previous?.folder === folder) {
      previous.changes.push(change);
    } else {
      windowGroups.push({ folder, changes: [change] });
    }
  }
  const groups = windowGroups.map((group) => Object.freeze({
    folder: group.folder,
    changes: Object.freeze(group.changes),
  }));
  const displayed = Math.min(matched, maximum);
  return Object.freeze({
    groups: Object.freeze(groups),
    orderedPaths,
    matched,
    displayed,
    offset,
    truncated: matched > displayed,
  });
}

export function workspaceChangeGroups(
  changes: readonly WorkspaceNativeChange[],
  filterText: string,
  filter: WorkspaceChangeFilter,
): readonly WorkspaceChangeGroup[] {
  const query = filterText.trim().toLocaleLowerCase();
  const grouped = new Map<string, WorkspaceNativeChange[]>();
  for (const change of changes) {
    if (filter !== "all" && change.code !== filter) continue;
    if (query && ![change.path, change.description, change.detail, change.status]
      .some((value) => value.toLocaleLowerCase().includes(query))) continue;
    const boundary = change.path.lastIndexOf("/");
    const folder = boundary === -1 ? "" : change.path.slice(0, boundary);
    const entries = grouped.get(folder) ?? [];
    entries.push(change);
    grouped.set(folder, entries);
  }
  const folders = [...grouped.keys()].sort((left, right) => {
    if (!left) return right ? -1 : 0;
    if (!right) return 1;
    return left.localeCompare(right, undefined, { sensitivity: "base" });
  });
  return Object.freeze(folders.map((folder) => Object.freeze({
    folder,
    changes: Object.freeze([...grouped.get(folder) ?? []].sort((left, right) => (
      left.path.localeCompare(right.path, undefined, { sensitivity: "base" })
    ))),
  })));
}

export function workspaceChangeNavigation(
  paths: readonly string[],
  selectedPath: string,
): WorkspaceChangeNavigation {
  if (paths.length === 0) return Object.freeze({ previous: null, next: null });
  const selectedIndex = paths.indexOf(selectedPath);
  if (selectedIndex === -1) {
    return Object.freeze({ previous: paths.at(-1) ?? null, next: paths[0] ?? null });
  }
  return Object.freeze({
    previous: selectedIndex > 0 ? paths[selectedIndex - 1] : null,
    next: selectedIndex < paths.length - 1 ? paths[selectedIndex + 1] : null,
  });
}

export function workspaceChangeRovingPath(
  visiblePaths: readonly string[],
  selectedPath: string,
  focusedPath: string,
): string {
  if (visiblePaths.includes(focusedPath)) return focusedPath;
  if (visiblePaths.includes(selectedPath)) return selectedPath;
  return visiblePaths[0] ?? "";
}

export function workspaceChangeFocusNavigation(
  paths: readonly string[],
  currentPath: string,
  key: "ArrowUp" | "ArrowDown" | "Home" | "End",
): string | null {
  if (paths.length === 0) return null;
  const currentIndex = Math.max(0, paths.indexOf(currentPath));
  const nextIndex = key === "Home"
    ? 0
    : key === "End"
      ? paths.length - 1
      : key === "ArrowUp"
        ? Math.max(0, currentIndex - 1)
        : Math.min(paths.length - 1, currentIndex + 1);
  return paths[nextIndex] ?? null;
}

export type WorkspaceExplorerNode = Readonly<{
  name: string;
  path: string;
  kind: "file" | "folder";
  explicit: boolean;
  children: readonly WorkspaceExplorerNode[];
}>;

export type WorkspaceExplorerRow = Readonly<{
  node: WorkspaceExplorerNode;
  level: number;
  expanded: boolean;
  parentPath: string | null;
}>;

export const WORKSPACE_EXPLORER_ROW_LIMIT = 500;

export type WorkspaceExplorerProjection = Readonly<{
  rows: readonly WorkspaceExplorerRow[];
  matched: number;
  truncated: boolean;
}>;

type MutableNode = {
  name: string;
  path: string;
  kind: "file" | "folder";
  explicit: boolean;
  children: MutableNode[];
};

function sortNodes(nodes: MutableNode[]): void {
  nodes.sort((left, right) => left.kind === right.kind
    ? left.name.localeCompare(right.name, undefined, { sensitivity: "base" })
    : left.kind === "folder" ? -1 : 1);
  nodes.forEach((node) => sortNodes(node.children));
}

function freezeNode(node: MutableNode): WorkspaceExplorerNode {
  return Object.freeze({
    ...node,
    children: Object.freeze(node.children.map(freezeNode)),
  });
}

export function workspaceExplorerTree(
  entries: readonly WorkspaceEntryChoice[],
): readonly WorkspaceExplorerNode[] {
  const roots: MutableNode[] = [];
  const byPath = new Map<string, MutableNode>();
  for (const entry of entries) {
    const parts = entry.value.split("/").filter(Boolean);
    let siblings = roots;
    let path = "";
    for (let index = 0; index < parts.length; index += 1) {
      const name = parts[index];
      path = path ? `${path}/${name}` : name;
      let node = byPath.get(path);
      if (!node) {
        node = {
          name,
          path,
          kind: index === parts.length - 1 ? entry.kind : "folder",
          explicit: index === parts.length - 1,
          children: [],
        };
        byPath.set(path, node);
        siblings.push(node);
      } else if (index === parts.length - 1) {
        node.kind = entry.kind;
        node.explicit = true;
      }
      siblings = node.children;
    }
  }
  sortNodes(roots);
  return Object.freeze(roots.map(freezeNode));
}

function nodeMatches(node: WorkspaceExplorerNode, query: string): boolean {
  return node.path.toLocaleLowerCase().includes(query)
    || node.children.some((child) => nodeMatches(child, query));
}

export function workspaceExplorerProjection(
  roots: readonly WorkspaceExplorerNode[],
  expanded: ReadonlySet<string>,
  filter: string,
  maximum = WORKSPACE_EXPLORER_ROW_LIMIT,
): WorkspaceExplorerProjection {
  if (!Number.isSafeInteger(maximum) || maximum < 1) throw new Error("The visible explorer-row bound was invalid.");
  const query = filter.trim().toLocaleLowerCase();
  const rows: WorkspaceExplorerRow[] = [];
  let matched = 0;
  const visit = (
    nodes: readonly WorkspaceExplorerNode[],
    level: number,
    parentPath: string | null,
  ) => {
    for (const node of nodes) {
      if (query && !nodeMatches(node, query)) continue;
      const open = node.kind === "folder" && (query.length > 0 || expanded.has(node.path));
      matched += 1;
      if (rows.length < maximum) rows.push(Object.freeze({ node, level, expanded: open, parentPath }));
      if (open) visit(node.children, level + 1, node.path);
    }
  };
  visit(roots, 1, null);
  return Object.freeze({
    rows: Object.freeze(rows),
    matched,
    truncated: matched > rows.length,
  });
}

export function visibleWorkspaceExplorerRows(
  roots: readonly WorkspaceExplorerNode[],
  expanded: ReadonlySet<string>,
  filter: string,
): readonly WorkspaceExplorerRow[] {
  return workspaceExplorerProjection(roots, expanded, filter).rows;
}

export function workspaceExplorerAncestorPaths(path: string): readonly string[] {
  const parts = path.split("/").filter(Boolean);
  return Object.freeze(parts.slice(0, -1).map((_, index) => parts.slice(0, index + 1).join("/")));
}

export function workspaceExplorerLocateFilter(
  roots: readonly WorkspaceExplorerNode[],
  selectedPath: string,
  maximum = WORKSPACE_EXPLORER_ROW_LIMIT,
): string {
  if (!selectedPath) return "";
  const expanded = new Set(workspaceExplorerAncestorPaths(selectedPath));
  const projection = workspaceExplorerProjection(roots, expanded, "", maximum);
  return projection.rows.some((row) => row.node.path === selectedPath) ? "" : selectedPath;
}

export type WorkspaceExplorerNavigation = Readonly<{
  focusPath: string;
  expandPath: string | null;
  collapsePath: string | null;
}>;

export type WorkspaceExplorerSummary = Readonly<{
  files: number;
  folders: number;
  total: number;
}>;

export function workspaceExplorerSummary(
  entries: readonly WorkspaceEntryChoice[],
): WorkspaceExplorerSummary {
  let files = 0;
  let folders = 0;
  const visit = (nodes: readonly WorkspaceExplorerNode[]) => {
    for (const node of nodes) {
      if (node.kind === "folder") folders += 1;
      else files += 1;
      visit(node.children);
    }
  };
  visit(workspaceExplorerTree(entries));
  return Object.freeze({ files, folders, total: files + folders });
}

export function workspaceExplorerNavigation(
  rows: readonly WorkspaceExplorerRow[],
  currentPath: string,
  key: "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Home" | "End",
): WorkspaceExplorerNavigation | null {
  if (rows.length === 0) return null;
  const currentIndex = Math.max(0, rows.findIndex((row) => row.node.path === currentPath));
  const current = rows[currentIndex];
  if (key === "Home" || key === "End" || key === "ArrowUp" || key === "ArrowDown") {
    const index = key === "Home"
      ? 0
      : key === "End"
        ? rows.length - 1
        : key === "ArrowUp"
          ? Math.max(0, currentIndex - 1)
          : Math.min(rows.length - 1, currentIndex + 1);
    return Object.freeze({ focusPath: rows[index].node.path, expandPath: null, collapsePath: null });
  }
  if (key === "ArrowRight") {
    if (current.node.kind !== "folder" || current.node.children.length === 0) {
      return Object.freeze({ focusPath: current.node.path, expandPath: null, collapsePath: null });
    }
    if (!current.expanded) {
      return Object.freeze({ focusPath: current.node.path, expandPath: current.node.path, collapsePath: null });
    }
    return Object.freeze({ focusPath: rows[currentIndex + 1]?.node.path ?? current.node.path, expandPath: null, collapsePath: null });
  }
  if (current.node.kind === "folder" && current.expanded) {
    return Object.freeze({ focusPath: current.node.path, expandPath: null, collapsePath: current.node.path });
  }
  return Object.freeze({ focusPath: current.parentPath ?? current.node.path, expandPath: null, collapsePath: null });
}
