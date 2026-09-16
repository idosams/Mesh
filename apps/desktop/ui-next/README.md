# Mesh UI next alpha foundation

This is the production React, Vite, Tailwind CSS, and shadcn-compatible interface. One React-owned
shell provides real pages and views for workspaces, import, current state, files, changes, review,
versions, destination updates, and restore. The former interface and its hidden controller markup
have been removed.

The production Tauri configuration serves the small coordinator and the built React island from
`apps/desktop/ui`. The application renders in an open Shadow DOM and receives twelve statically
slotted surface hosts. React has no native command calls, network client, or storage access. The
verified coordinator supplies frozen bounded projections and current action availability. React
emits typed intents; the coordinator rechecks the mounted generation, exact workspace binding, and
current action before invoking a native operation. Malformed or stale input is refused.

## Structure

- `src/atoms`: source-owned shadcn-style primitives and variants.
- `src/molecules`: small combinations with one user purpose.
- `src/models`: frozen view models and typed user intents; no native command authority.
- `src/organisms`: complete workflow regions, starting with artifact review.
- `src/layouts`: application framing and spatial rules.
- `src/pages`: page composition only.
- `src/views`: route-level workflow groupings and island slots.

## Offline verification

From `apps/desktop`:

```sh
npm --prefix ui-next ci --offline --ignore-scripts --no-audit --no-fund
npm run ui:next:check
npm run ui:next:build:island
```

The dependency URLs and integrity hashes are pinned by `ui-next/package-lock.json`. `esbuild` and
`jiti` are direct pins because unconstrained offline resolution otherwise selects newer transitive
versions that are not in the verified cache.

## Authority boundary

The coordinator in `apps/desktop/ui/app.js` owns native command sequencing and source state. Every
visible control is React-owned and sends a closed typed intent. An intent is accepted only for the
currently mounted projection generation and the exact current workspace, selection, and action.
Long-running native operations recheck the same physical and logical authority after every await.

Pending first mounts and failed renders stay hidden so the page-level loading or Reload state remains
usable. Same-workspace field echoes retain a narrowly bounded interaction generation where necessary
to prevent focus loss without widening mutation authority. The packaged proof exercises onboarding,
review, saved versions, private export, and agent handoff through the React controls.

Restore receives bounded file/version summaries and emits preview, apply, and one-shot undo intents.
React labels common text, PDF, Word, PowerPoint, and Excel filenames for nondeveloper recognition,
but it receives no direct filesystem access. The coordinator retains exact workspace, selection,
inspected-content, and undo freshness checks.

The review organism receives one frozen `ReviewWorkbenchModel` and emits typed intents. It owns only
presentation state such as the selected file and comparison mode. Inspection, record, approval, and
export remain enabled only while the coordinator has the matching current action; they never become
optimistic React state and never call Tauri directly.

`review-workbench-adapter.ts` is the first strangler seam. It accepts the daemon's bounded review
projection as `unknown`, rejects incomplete projections, validates exact identities and content
summaries, and returns a deeply frozen workbench model. It receives button availability as a
separate coordinator-owned input and never infers approval authority from presentation data.
The same seam now preserves canonical line numbers and hunk boundaries for familiar split and inline
text diffs. PDF and Office items default to an on-demand visual comparison: the coordinator renders
each exact saved side through the existing native renderer, revalidates its version and content
digest, and returns only a bounded PNG preview to the island. PDF pages can be compared side by side;
PowerPoint, Word, and Excel use representative native previews and keep exact-copy inspection for
complete formatting and interactions. The same closed preview envelope carries bounded inert text
and section identities, so the Content changes view can show familiar split or inline differences
for PDF page text, PowerPoint slides, Word paragraphs and tables, and Excel cells and formulas.
Both React input envelopes use exact closed key sets and reject control or bidirectional-override
text. Extracted sections also retain their format structure: PDF page identity, ordered PowerPoint
slides, ordered Word sections, and unique Excel sheet labels are independently checked at the
presentation boundary. No preview or extracted-content result can authorize approval. Local
navigation runs through a pure reducer that keeps every successor model frozen; native intents do
not optimistically alter review or approval state.

The alpha workbench exposes explicit keyboard focus, 44-pixel controls, screen-reader change status,
bounded progress/error announcements, and component boundaries with at least 3:1 contrast against
every adjacent surface token. A failed one-sided document extraction offers a named retry; two
incompatible safe extraction sources stop with Visual/exact-copy guidance instead of looping on the
same load action. Changed-file and comparison choices use one Tab stop per group with deterministic
arrow, Home, and End movement, so a large review bundle does not become a long keyboard trap.

The cached dependency set does not currently include Radix UI or Lucide. The scaffold therefore
uses the shadcn source-ownership pattern for Button, Badge, Card, and SegmentedControl without
claiming that dialog, menu, tooltip, or icon primitives have been migrated. Add and review those
packages before copying in interaction-heavy shadcn components.
