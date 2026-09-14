import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

type Options = { maxRequests: number; maxInputChars: number; samplesPerFinding: number };
type Preview = { version: string; previewDigest: string; provider: string; model: string; endpoint: string; findingCount: number; completedFindings: number; pendingFindings: number; estimatedRequests: number; estimatedInputChars: number; sampledEvidence: number; overviewPending: boolean; overviewPlanned: boolean; samplingPolicy: string; dataCategories: string[]; options: Options };
type Annotation = { findingId: string; evidenceIds: string[]; explanation: string; proposedCause?: string; recommendation: string; verification: string; suggestedTeam: string; model: string; generatedAtMs: number; sampleCount: number; evidenceTotal: number; inputTokens?: number; outputTokens?: number };
type Overview = { summary: string; prioritizedFindingIds: string[]; limitations: string[]; model: string; generatedAtMs: number; includedFindingCount: number; totalFindingCount: number; sampledEvidenceCount: number; partialCoverage: boolean; inputTokens?: number; outputTokens?: number };
type Status = { version: string; status: string; provider: string; model: string; findingCount: number; completedFindings: number; requests: number; inputChars: number; inputTokens?: number; outputTokens?: number; overview?: Overview; error?: string; rows: Annotation[]; exportGenerationVersion?: string; preservedGeneration?: Status };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const tokens = (value?: number) => value == null ? "Unavailable" : value.toLocaleString();

export default function AuditReportAiPanel({ entityId, entityKind = "report", active }: { entityId: string; entityKind?: "report" | "comparison"; active: boolean }) {
  const [options, setOptions] = useState<Options>({ maxRequests: 10, maxInputChars: 100_000, samplesPerFinding: 3 });
  const [preview, setPreview] = useState<Preview>();
  const [status, setStatus] = useState<Status>();
  const [offset, setOffset] = useState(0);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState<string>();
  const [error, setError] = useState<string>();
  const generation = useRef(0);
  const request = useRef<string | undefined>(undefined);
  const visible = useRef(active);
  const currentTarget = useRef(entityId);
  visible.current = active;
  currentTarget.current = entityId;
  const commands = entityKind === "comparison" ? { get: "get_audit_report_comparison_ai", preview: "preview_audit_report_comparison_ai", run: "run_audit_report_comparison_ai", key: "comparisonId" as const } : { get: "get_audit_report_ai", preview: "preview_audit_report_ai", run: "run_audit_report_ai", key: "reportId" as const };
  const target = { [commands.key]: entityId };

  const load = async (nextOffset = 0) => {
    const version = ++generation.current;
    try {
      const saved = await invoke<Status | null>(commands.get, { ...target, offset: nextOffset, limit: 100 });
      if (version === generation.current && visible.current && currentTarget.current === entityId) { setStatus(saved ?? undefined); setOffset(nextOffset); }
    } catch (caught) { if (version === generation.current && visible.current) setError(message(caught)); }
  };
  useEffect(() => {
    setPreview(undefined); setStatus(undefined); setError(undefined); setBusy(false); setPhase(undefined); setOffset(0);
    if (active) void load();
    return () => {
      generation.current++;
      const id = request.current; request.current = undefined;
      if (id) void invoke("cancel_audit_report", { requestId: id }).catch(() => {});
    };
  }, [entityId, entityKind, active]);
  useEffect(() => {
    if (!active || !isTauri()) return;
    let alive = true; let dispose: (() => void) | undefined;
    void listen<{ requestId: string; phase: string; completed: number; total?: number }>("audit-report-progress", ({ payload }) => {
      if (payload.requestId === request.current) setPhase(`${payload.phase} · ${payload.completed.toLocaleString()}${payload.total != null ? ` / ${payload.total.toLocaleString()}` : ""}`);
    }).then((unlisten) => { if (alive) dispose = unlisten; else unlisten(); }).catch((caught) => { if (alive) setError(message(caught)); });
    return () => { alive = false; dispose?.(); };
  }, [active]);

  const review = async () => {
    const version = ++generation.current;
    setBusy(true); setError(undefined); setPreview(undefined);
    try {
      const result = await invoke<Preview>(commands.preview, { ...target, options });
      if (version === generation.current && visible.current) setPreview(result);
    } catch (caught) { if (version === generation.current && visible.current) setError(message(caught)); }
    finally { if (version === generation.current && visible.current) setBusy(false); }
  };
  const run = async () => {
    if (!preview) return;
    const id = crypto.randomUUID(); request.current = id;
    setBusy(true); setError(undefined); setPhase("Preparing AI explanations…");
    try {
      const result = await invoke<Status>(commands.run, { request: { requestId: id, ...target, expectedVersion: preview.version, expectedPreviewDigest: preview.previewDigest, options } });
      if (request.current === id && visible.current) { setStatus(result); setOffset(0); setPreview(undefined); }
    } catch (caught) { if (request.current === id && visible.current) setError(message(caught)); }
    finally { if (request.current === id && visible.current) { request.current = undefined; setBusy(false); setPhase(undefined); } }
  };
  const cancel = async () => {
    const id = request.current; if (!id) return;
    setPhase("Cancelling AI generation…");
    try { await invoke("cancel_audit_report", { requestId: id }); }
    catch (caught) { if (request.current === id && visible.current) setError(message(caught)); }
  };
  const update = (key: keyof Options, value: string) => { setOptions((current) => ({ ...current, [key]: Number(value) })); setPreview(undefined); };

  return <section className="audit-report-ai" aria-label="Optional AI explanations">
    <h3>Optional AI explanations</h3>
    <p>Generate explanations from measured findings and bounded captured examples. All findings and affected URLs remain available without AI.</p>
    <fieldset disabled={busy}><legend>Generation budget</legend>
      <label>Maximum requests<input aria-label="Audit AI maximum requests" type="number" min={1} max={100} value={options.maxRequests} onChange={(event) => update("maxRequests", event.target.value)} /></label>
      <label>Total input characters<input aria-label="Audit AI total input characters" type="number" min={2000} max={2_000_000} step={1000} value={options.maxInputChars} onChange={(event) => update("maxInputChars", event.target.value)} /></label>
      <label>Examples per finding<input aria-label="Audit AI samples per finding" type="number" min={1} max={10} value={options.samplesPerFinding} onChange={(event) => update("samplesPerFinding", event.target.value)} /></label>
      <button onClick={() => void review()}>Review AI data and budget</button>
    </fieldset>
    {preview ? <div className="audit-ai-preview"><h4>Before sending to AI</h4>
      <p>{preview.provider} · {preview.model} · {preview.endpoint}</p>
      <p>{preview.completedFindings.toLocaleString()} / {preview.findingCount.toLocaleString()} findings already explained. This run plans {preview.estimatedRequests.toLocaleString()} requests with {preview.estimatedInputChars.toLocaleString()} input characters and {preview.sampledEvidence.toLocaleString()} evidence examples.</p>
      <p>Overview pending: {preview.overviewPending ? "Yes" : "No"} · Overview planned: {preview.overviewPlanned ? "Yes" : "No"}</p>{preview.samplingPolicy ? <p>Sampling policy: {preview.samplingPolicy}</p> : null}
      <ul>{preview.dataCategories.map((category) => <li key={category}>{category}</li>)}</ul>
      <p>Request totals are estimates; retries consume the same budget. Provider pricing is unavailable here. Credentials come from Settings &gt; AI.</p>
      <button disabled={busy || !preview.estimatedRequests} onClick={() => void run()}>{preview.overviewPending ? "Send evidence and generate explanations and overview" : "Send evidence and generate explanations"}</button>
    </div> : null}
    {request.current ? <button onClick={() => void cancel()}>Cancel AI generation</button> : null}
    {phase ? <p role="status">{phase}</p> : null}
    {error ? <p role="alert" className="error-bar">{error}</p> : null}
    {status ? <>{(() => { const displayed = status.preservedGeneration ?? status; return <><p role="status">AI generation: {displayed.status} · {displayed.completedFindings.toLocaleString()} / {displayed.findingCount.toLocaleString()} findings explained. Version {displayed.version} · {displayed.provider} · {displayed.model}.</p>
      {status.preservedGeneration ? <section className="audit-ai-preserved"><h4>Preserved export generation</h4><p>Latest generation {status.version} · {status.provider} · {status.model} is {status.status}. Export uses preserved version {displayed.version} · {displayed.provider} · {displayed.model}.</p>{status.error ? <p role="alert">Latest generation failed: {status.error}</p> : null}</section> : null}
      {!status.preservedGeneration && status.error ? <p role="alert">{status.error}</p> : null}
      <p>{displayed.requests.toLocaleString()} requests · {displayed.inputChars.toLocaleString()} input characters · Input tokens: {tokens(displayed.inputTokens)} · Output tokens: {tokens(displayed.outputTokens)}</p>
      {displayed.overview ? <section className="audit-ai-overview"><h4>AI overview</h4><p>{displayed.overview.summary}</p><p>Included findings: {displayed.overview.includedFindingCount.toLocaleString()} / {displayed.overview.totalFindingCount.toLocaleString()} · Sampled evidence: {displayed.overview.sampledEvidenceCount.toLocaleString()} · {displayed.overview.partialCoverage ? "Partial coverage" : "Full included coverage"}</p><p>Prioritized findings: {displayed.overview.prioritizedFindingIds.join(", ") || "None"}</p><details><summary>Overview limitations</summary>{displayed.overview.limitations.map((limitation) => <p key={limitation}>{limitation}</p>)}</details></section> : displayed.completedFindings === displayed.findingCount ? <p role="status">Finding explanations are complete; an overview is pending.</p> : null}
      <p>AI interpretations and proposed causes are unverified. The measured counts and full evidence remain the source of truth.</p>
      {displayed.rows.map((annotation) => <details key={annotation.findingId} className="audit-ai-annotation"><summary>{annotation.findingId} · AI explanation</summary>
        <p>{annotation.explanation}</p>{annotation.proposedCause ? <p><strong>Proposed cause — unverified:</strong> {annotation.proposedCause}</p> : null}
        <p><strong>Recommendation:</strong> {annotation.recommendation}</p><p><strong>Verification:</strong> {annotation.verification}</p>
        <p>Suggested team: {annotation.suggestedTeam} · Examples: {annotation.sampleCount} / {annotation.evidenceTotal.toLocaleString()} occurrences · Model: {annotation.model}</p>
        <p>Evidence references: {annotation.evidenceIds.join(", ") || "No examples available"}</p>
      </details>)}
      {displayed.completedFindings > 100 ? <div className="audit-evidence-pagination"><span>{offset + 1}–{offset + displayed.rows.length} of {displayed.completedFindings}</span><button disabled={!offset || busy} onClick={() => void load(offset - 100)}>Previous explanations</button><button disabled={busy || offset + 100 >= displayed.completedFindings} onClick={() => void load(offset + 100)}>Next explanations</button></div> : null}
    </>; })()}</> : null}
  </section>;
}
