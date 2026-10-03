import { RemoteExecution, type RecordedExecution } from "./remote-execution";
import { RemoteObservation } from "./remote-observation";
import { RemoteReviewPanels, RemoteSavedResults, type RemoteReviewPin, type RemoteReviewQueue, type RemoteProjectWorkflow } from "./remote-fleet-reviews";
import { useEffect, useState } from "react";
import { useTranslation } from "../lib/localization";
import { FleetReviewPanels, FleetSavedResults, type FleetReviewPin, type FleetReviewQueue, type FleetReviewPersistence } from "./fleet-reviews";
import { Button } from "../atoms/button";

type Source = { id: string; root: string; savedVersion: string | null; detached?: boolean };
type Lane = { id: string; parent: string | null; sourceProject: string | null; goal: string; provider: string; base: string; allocated: boolean; run: { id: string; state: string; remote?: { assignment: string; worker: string; leaseSequence: string; leaseUntil: string } | null } | null };
type ProviderPolicy = { coordinator: string; providers: string[] };
type Fleet = { policy?: ProviderPolicy | null; objective: string; ownership: string; cancelled: boolean; lanes: Lane[] };
type Worker = { lane: string; run: string; observedAt: string; activity: string | null; outcome: boolean | null; events: string };
type Activity = { objective: string; status: string; stopRequested: boolean; observedAt: string | null; workers: Worker[] };
type Projection = { remoteObservations?: Record<string, { run: string; assignment: string; worker: string; value: RecordedExecution | null; busy: boolean; error: string }>; remoteProjectWorkflow?:RemoteProjectWorkflow; remoteReviewPersistence?: FleetReviewPersistence; remoteReviewPins?: RemoteReviewPin[]; remoteReviewQueues?: Record<string, RemoteReviewQueue>; remoteReviewNotice?: string; reviewOutbox?: { entries: { kind: string; objective: string; selection: { lane: string; checkpoint: string; version: string }; input: { request: string; operation?: string; message?: string; expected_revision?: string; checkpoint?: string | null; version?: string | null; bundle?: string | null } }[]; busy: boolean; loaded: boolean; error: string }; reviewPersistence?: FleetReviewPersistence; reviewQueues?: Record<string, FleetReviewQueue>; reviewPins?: FleetReviewPin[]; reviewNotice?: string; fleets: Fleet[]; activity: Activity[]; pending: { policy?: ProviderPolicy; id: string; version: string; goal: string; limits: { lanes: number; concurrency: number; depth: number } } | null; busy: boolean; error: string; feedback: string; available: boolean };
const empty: Projection = { fleets: [], activity: [], pending: null, busy: false, error: "", feedback: "", available: false };
const send = (detail: Record<string, unknown>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));
const states: Record<string, string> = { launching: "Starting", running: "Working", waiting: "Waiting", reconciling: "Needs recovery", stopping: "Stop requested · ownership reserved", succeeded: "Execution completed", failed: "Execution failed", cancelled: "Cancelled" };
const age = (value: string, t: ReturnType<typeof useTranslation>) => {
  const milliseconds = Number(value);
  return Number.isSafeInteger(milliseconds) && milliseconds <= Date.now()
    ? <>{t("Observed")} {Math.floor((Date.now() - milliseconds) / 1000)} {t("seconds ago")}</>
    : t("Observation time unavailable");
};
export function Fleets({ projects, histories, sourceError }: { projects: Source[]; histories: Record<string, { versions: string[] }>; sourceError: string }) {
  const t = useTranslation();
  const [projection, setProjection] = useState<Projection>(empty);
  const [input, setInput] = useState({ id: "", version: "" });
  const [goal, setGoal] = useState("");
  const [lanes, setLanes] = useState("4");
  const [concurrency, setConcurrency] = useState("2");
  const [depth, setDepth] = useState("1");
  const [coordinator, setCoordinator] = useState("codex");
  const [providers, setProviders] = useState<string[]>(["codex"]);
  useEffect(() => {
    const update = (event: Event) => setProjection((event as CustomEvent<Projection>).detail);
    document.addEventListener("mesh:fleets-projection", update);
    document.dispatchEvent(new CustomEvent("mesh:fleets-visible", { detail: true }));
    return () => { document.removeEventListener("mesh:fleets-projection", update); document.dispatchEvent(new CustomEvent("mesh:fleets-visible", { detail: false })); };
  }, []);
  const source = projects.find(project => project.id === input.id);
  const versions = [...new Set([...(source?.savedVersion ? [source.savedVersion] : []), ...(histories[input.id]?.versions ?? [])])];
  const disabled = projection.busy || !projection.available;
  const formDisabled = !projection.available || Boolean(projection.pending);
  const numbersValid = [lanes, concurrency, depth].every(value => /^(0|[1-9][0-9]{0,3})$/.test(value))
    && Number(lanes) >= 1 && Number(lanes) <= 1024 && Number(concurrency) >= 1 && Number(concurrency) <= Math.min(64, Number(lanes)) && Number(depth) <= 32;
  const goalValid = goal.trim().length > 0 && new TextEncoder().encode(goal).length <= 8192 && !/[\u0000\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(goal);
  return <section aria-label={t("Agent fleets")} data-mesh-proof="agent-fleets" className="grid gap-4 rounded-lg border border-border p-4">
    <div><h3 className="text-xl font-semibold">{t("Agent fleets")}</h3>
      <p className="text-sm text-muted-foreground">{t("Optional agents work in independent lanes created from a saved version. Keep using your original project and harness as usual.")}</p></div>
    <RemoteObservation fleets={projection.fleets} projects={sourceError ? [] : projects} histories={histories} />
    <details><summary className="cursor-pointer font-medium">{t("Provision a fleet from saved work")}</summary>
      <div className="mt-3 grid gap-3">
        <label className="grid gap-1 text-sm">{t("Project")}<select dir="ltr" className="min-h-11 rounded border bg-background p-2" value={input.id} disabled={formDisabled} onChange={event => setInput({ id: event.target.value, version: "" })}>
          <option value="">{t("Choose project")}</option>{projects.filter(project => !project.detached && project.savedVersion).map(project => <option key={project.id} value={project.id}>{project.root}</option>)}
        </select></label>
        <label className="grid gap-1 text-sm">{t("Starting saved version")}<select dir="ltr" className="min-h-11 min-w-0 rounded border bg-background p-2" value={input.version} disabled={formDisabled} onChange={event => setInput({ ...input, version: event.target.value })}>
          <option value="">{t("Choose exact saved version")}</option>{input.version && !versions.includes(input.version) && <option value={input.version}>{input.version} · {t("retained selection")}</option>}{versions.map(version => <option key={version} value={version}>{version}</option>)}
        </select></label>
        <p className="text-xs text-muted-foreground">{t("Use \u201cShow latest versions\u201d on the project below to make older versions available here.")}</p>
        <label className="grid gap-1 text-sm">{t("What should the agents accomplish?")}<textarea dir="auto" className="min-h-24 rounded border bg-background p-2" value={goal} maxLength={8192} disabled={formDisabled} onChange={event => setGoal(event.target.value)} /></label>
        <div className="grid gap-3 sm:grid-cols-3">{[{ label: "Maximum lanes, including coordinator", value: lanes, setter: setLanes, min: 1, max: 1024 }, { label: "Agents running at once", value: concurrency, setter: setConcurrency, min: 1, max: 64 }, { label: "Delegation depth", value: depth, setter: setDepth, min: 0, max: 32 }].map(field => <label key={field.label} className="grid gap-1 text-sm">{t(field.label)}<input type="number" className="min-h-11 rounded border bg-background p-2" min={field.min} max={field.max} value={field.value} disabled={formDisabled} onChange={event => field.setter(event.target.value)} /></label>)}</div>
        <label className="grid gap-1 text-sm">{t("Coordinator provider")}<select dir="ltr" value={coordinator} disabled={formDisabled} onChange={event => { const next = event.target.value; setCoordinator(next); setProviders(previous => [...new Set([...previous, next])].sort()); }} className="min-h-11 rounded border bg-background p-2"><option value="codex">Codex</option><option value="claude">Claude</option></select></label>
        <fieldset disabled={formDisabled} className="grid gap-2"><legend>{t("Allowed providers")}</legend>{["codex", "claude"].map(provider => <label key={provider} className="flex min-h-11 items-center gap-2"><input type="checkbox" checked={providers.includes(provider)} disabled={formDisabled || provider === coordinator} onChange={event => setProviders(previous => event.target.checked ? [...new Set([...previous, provider])].sort() : previous.filter(value => value !== provider))} /><bdi dir="ltr">{provider}</bdi></label>)}</fieldset>
        <p className="text-xs text-muted-foreground">{t("All selected providers need an installed executable and account. Provisioning creates the lane; Start agents begins provider usage. Failed or uncertain attempts are not retried automatically.")}</p>
        <Button disabled={disabled || Boolean(projection.pending) || Boolean(projection.error) || Boolean(sourceError) || !source || Boolean(source.detached) || !input.version || !goalValid || !numbersValid}
          onClick={() => send({ type: "provision", ...input, goal, lanes, concurrency, depth, policy: { coordinator, providers } })}>{t("Provision fleet")}</Button>
      </div>
    </details>
    {projection.pending && <div role="status" className="grid gap-2 text-sm"><p>{t("Provisioning not yet confirmed. This request keeps the same starting version and limits.")}</p><p dir="auto" className="whitespace-pre-wrap break-words">{projection.pending.goal}</p><p className="break-all">{t("Version")}: <bdi dir="ltr">{projection.pending.version}</bdi></p>
      <FleetPolicy policy={projection.pending.policy} />
      <p>{projection.pending.limits.lanes} {t("lanes")} · {projection.pending.limits.concurrency} {t("agents at once")} · {t("depth")} {projection.pending.limits.depth}</p>
      <Button disabled={disabled} onClick={() => send({ type: "retry-provision" })}>{t("Retry this provisioning request")}</Button></div>}
    {projection.error && <p role="alert" className="text-sm">{t(projection.error)}</p>}
    {projection.feedback && <p role="status" className="break-words text-sm">{t(projection.feedback)}</p>}
    <Button variant="secondary" disabled={disabled} onClick={() => send({ type: "refresh" })}>{t("Refresh fleets")}</Button>
    {!projection.fleets.length && <p className="text-sm text-muted-foreground">{t(Object.keys(projection.remoteReviewQueues ?? {}).length ? "Fleet status has not loaded for these saved reviews." : projection.error ? "Saved fleets could not be loaded." : "No fleets yet. Manual work and externally run harnesses remain available below.")}</p>}
    {projection.reviewOutbox && <FleetPendingReviewOperations value={projection.reviewOutbox} />}
    <FleetReviewPanels pins={projection.reviewPins ?? []} notice={projection.reviewNotice ?? ""} persistence={projection.reviewPersistence} />
    <RemoteReviewPanels workflow={projection.remoteProjectWorkflow} pins={projection.remoteReviewPins ?? []} notice={projection.remoteReviewNotice ?? ""} persistence={projection.remoteReviewPersistence} />
    <FleetCards projection={projection} projects={projects} disabled={disabled} />
  </section>;
}

export function FleetCards({ projection, projects, disabled }: { projection: Projection; projects: Source[]; disabled: boolean }) {
  const t = useTranslation();
  return (
    <div className="grid items-start gap-4 xl:grid-cols-2">
      {Object.entries(projection.remoteReviewQueues ?? {}).filter(([objective]) => !projection.fleets.some(fleet => fleet.objective === objective)).map(([objective, queue]) => <article key={objective} className="grid min-w-0 gap-3 rounded border border-border p-3">
        <h4 className="font-semibold">{t("Saved remote reviews")}</h4>
        <p className="text-sm">{t("Fleet status has not loaded for these saved reviews.")}</p>
        <details className="break-all text-xs"><summary>{t("Fleet identity")}</summary><bdi dir="ltr">{objective}</bdi></details>
        <RemoteSavedResults objective={objective} queue={queue} available={projection.available && (projection.remoteReviewPersistence?.editable ?? true)} />
      </article>)}
      {projection.fleets.map(fleet => {
      const activity = fleet.ownership === "current-host" ? projection.activity.find(row => row.objective === fleet.objective) : undefined;
      const current = fleet.ownership === "current-host";
      const ready = Boolean(fleet.policy) && current && !fleet.cancelled && !activity && fleet.lanes.length > 0 && fleet.lanes.every(lane => lane.allocated && !lane.run);
      return <article key={fleet.objective} className="grid min-w-0 gap-3 rounded border border-border p-3">
        <h4 dir="auto" className="whitespace-pre-wrap break-words font-semibold">{fleet.lanes.find(lane => lane.parent === null)?.goal ?? t("Retained fleet")}</h4>
        <details className="break-all text-xs"><summary>{t("Fleet identity")}</summary><bdi dir="ltr">{fleet.objective}</bdi></details>
        <p className="text-sm">{projection.error ? <>{t("Status may be out of date")} · </> : ""}{t(!current ? fleet.ownership === "restored-unattached" ? "Saved state · workers need recovery before execution" : "Fleet unavailable · retained work needs recovery" : fleet.cancelled ? "Stopped scheduling · worker ownership retained" : activity?.status === "needs-attention" ? "Needs attention · new agents will not start" : activity?.stopRequested ? "Stop requested" : activity ? "Monitoring agents" : fleet.lanes.some(lane => !lane.allocated) ? "Lane allocation incomplete" : fleet.lanes.some(lane => lane.run) ? "Saved run status · no live observations" : "Provisioned · agents have not started")}</p>
        <FleetPolicy policy={fleet.policy} />
        <RemoteSavedResults objective={fleet.objective} queue={projection.remoteReviewQueues?.[fleet.objective]} available={projection.available && (projection.remoteReviewPersistence?.editable ?? true)} />
        <div className="flex flex-wrap gap-2"><Button disabled={disabled || Boolean(projection.error) || !ready} onClick={() => send({ type: "start", objective: fleet.objective })}>{t("Start agents")}</Button>
          <Button variant="secondary" disabled={disabled || !current || fleet.cancelled} onClick={() => send({ type: "stop", objective: fleet.objective })}>{t("Stop agents")}</Button></div>
        <p className="text-xs text-muted-foreground">{activity?.observedAt ? age(activity.observedAt, t) : t("No worker observations in this app session")}</p>
        <ul className="grid max-h-[36rem] gap-3 overflow-auto">{fleet.lanes.map(lane => {
          const worker = current && lane.run && !lane.run.remote ? activity?.workers.find(worker => worker.lane === lane.id && worker.run === lane.run?.id) : null;
          const remoteRow = projection.remoteObservations?.[`${fleet.objective}/${lane.id}`];
          const remote = remoteRow && lane.run?.remote && remoteRow.run === lane.run.id
            && remoteRow.assignment === lane.run.remote.assignment && remoteRow.worker === lane.run.remote.worker ? remoteRow : null;
          return <li key={lane.id} className="grid gap-1 rounded border border-border p-2 text-sm">
            <p className="font-medium">{t(lane.parent ? "Worker lane" : "Coordinator lane")} · <bdi dir="ltr">{lane.provider}</bdi></p><p dir="auto" className="whitespace-pre-wrap break-words">{lane.goal}</p>
            <p>{lane.run ? <>{!worker && <>{t("Last saved state")}: </>}{t(states[lane.run.state])}</> : t(!current ? "Saved lane · recovery required" : lane.allocated ? "Waiting to start" : "Allocation needs attention")}</p>
            {lane.run?.remote && <div className="grid gap-1 text-xs">
              <p>{t("Remote worker · last coordinator record")}</p>
              {remote?.value ? <RemoteExecution value={remote.value} /> : <p>{t("Remote execution has not been observed in this view")}</p>}
              {remote?.busy && <p role="status">{t("Refreshing this remote lane")}</p>}
              {remote?.error && <p role="status">{t(remote.error)} {t("Check the saved connection for this lane.")}</p>}
              <details className="break-all"><summary>{t("Remote assignment")}</summary>
                <p>{t("Assignment")}: <bdi dir="ltr">{lane.run.remote.assignment}</bdi></p>
                <p>{t("Worker identity")}: <bdi dir="ltr">{lane.run.remote.worker}</bdi></p>
                <p>{t("Recorded lease revision")}: <bdi dir="ltr">{lane.run.remote.leaseSequence}</bdi></p>
                <p>{t("Recorded lease deadline (Unix ms)")}: <bdi dir="ltr">{lane.run.remote.leaseUntil}</bdi></p>
                <p>{t("A lease deadline does not prove the worker stopped.")}</p>
              </details>
            </div>}
            {worker && <p className="text-xs">{age(worker.observedAt, t)} · {worker.activity ? <bdi dir="ltr">{worker.activity}</bdi> : t("No activity reported")} · {worker.events} {t("events")}</p>}
            <FleetSavedResults objective={fleet.objective} lane={lane.id} queue={projection.reviewQueues?.[`${fleet.objective}/${lane.id}`]} available={projection.available && fleet.ownership !== "unavailable"} />
            <details className="break-all text-xs"><summary>{t("Lane and starting version")}</summary><p>{t("Lane")}: <bdi dir="ltr">{lane.id}</bdi></p>{lane.parent && <p>{t("Parent lane")}: <bdi dir="ltr">{lane.parent}</bdi></p>}<p>{t("Starting version")}: <bdi dir="ltr">{lane.base}</bdi></p><p>{t("Project")}: {lane.sourceProject ? <bdi dir="ltr">{projects.find(project => project.id === lane.sourceProject)?.root ?? lane.sourceProject}</bdi> : t("No attached source")}</p></details>
          </li>;
        })}</ul>
        <p className="text-xs text-muted-foreground">{t("Agent completion does not approve changes to main. Saved reviews can be pinned above. Approval and worker recovery are not available yet.")}</p>
      </article>;
    })}</div>
  );
}

export function FleetPendingReviewOperations({ value }: { value: NonNullable<Projection["reviewOutbox"]> }) {
  const t = useTranslation();
  return <section className="grid gap-2 rounded border border-border p-3" aria-label={t("Pending review operations")}>
      <h4 className="font-semibold">{t("Pending review operations")}</h4>
      <p className="text-xs">{t("Saved inputs survive closed panels and restarts. Refresh reads only; retry submits the exact saved request.")}</p>
      <Button variant="secondary" disabled={value?.busy} onClick={() => send({ type: "refresh-review-outbox" })}>{t("Refresh pending operations")}</Button>
      {value.error && <p role="alert">{t(value.error)}</p>}
      {value.entries.map(entry => <div className="grid gap-2 text-xs" key={entry.input.operation ?? entry.input.request}>
        <p>{t(entry.kind === "change" ? "Change request" : "Request decision")} · <bdi dir="ltr">{entry.selection.lane}</bdi> · <bdi dir="ltr">{entry.selection.checkpoint}</bdi></p>
        <details className="break-all"><summary>{t("Exact pending inputs")}</summary><p>{t("Fleet")}: <bdi dir="ltr">{entry.objective}</bdi></p><p>{t("Version")}: <bdi dir="ltr">{entry.selection.version}</bdi></p><p>{t("Request")}: <bdi dir="ltr">{entry.input.request}</bdi></p>{entry.input.message && <p dir="auto" className="whitespace-pre-wrap">{entry.input.message}</p>}{entry.kind === "decision" && <><p>{t("Expected decision revision")}: <bdi dir="ltr">{entry.input.expected_revision}</bdi></p><p>{t("Proposed checkpoint")}: {entry.input.checkpoint === null ? t("Reopen request") : <bdi dir="ltr">{entry.input.checkpoint}</bdi>}</p><p>{t("Proposed version")}: {entry.input.version === null ? t("None") : <bdi dir="ltr">{entry.input.version}</bdi>}</p><p>{t("Proposed review")}: {entry.input.bundle === null ? t("None") : <bdi dir="ltr">{entry.input.bundle}</bdi>}</p></>}</details>
        <Button variant="secondary" disabled={value?.busy} onClick={() => send({ type: "retry-pending-review", operation: entry.input.operation ?? entry.input.request })}>{t("Retry this exact operation")}</Button>
        {entry.kind === "decision" && <Button variant="secondary" disabled={value?.busy} onClick={() => send({ type: "reconcile-pending-review", operation: entry.input.operation! })}>{t("Read current decisions and stop retrying this operation")}</Button>}
      </div>)}
      {value.loaded && !value.entries.length && <p className="text-xs">{t("No pending operations were returned by the last successful read.")}</p>}
    </section>;
}

function FleetPolicy({ policy }: { policy?: ProviderPolicy | null }) {
  const t = useTranslation();
  return policy ? <div className="text-sm"><p>{t("Coordinator provider")}: <bdi dir="ltr">{policy.coordinator}</bdi></p><p>{t("Allowed providers")}: <bdi dir="ltr">{policy.providers.join(", ")}</bdi></p></div> : <p className="text-sm">{t("Provider choices unavailable. Refresh before starting agents.")}</p>;
}
