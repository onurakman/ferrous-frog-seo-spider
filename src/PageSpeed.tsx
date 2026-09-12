import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { Check, RefreshCw, Settings, Trash2 } from "lucide-react";

export type PageSpeedStrategy = "mobile" | "desktop";
export type PageSpeedSnapshot = {
  strategy: PageSpeedStrategy;
  requestedUrl: string;
  completedAtMs: number;
  finalUrl?: string | null;
  fetchedAt?: string | null;
  lighthouseVersion?: string | null;
  performanceScore?: number | null;
  accessibilityScore?: number | null;
  bestPracticesScore?: number | null;
  seoScore?: number | null;
  lcpMs?: number | null;
  tbtMs?: number | null;
  cls?: number | null;
};

export type FieldFormFactor = "phone" | "desktop" | "tablet";
export type FieldVitalsSnapshot = {
  formFactor: FieldFormFactor;
  requestedUrl: string;
  completedAtMs: number;
  hasData: boolean;
  lcpMsP75?: number | null;
  clsP75?: number | null;
  inpMsP75?: number | null;
  fcpMsP75?: number | null;
  ttfbMsP75?: number | null;
  collectionPeriodStart?: string | null;
  collectionPeriodEnd?: string | null;
};

type CredentialStatus = { keySaved: boolean; keyringAvailable: boolean; message?: string | null };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

// Keep credential operations alive when the Settings portal closes; never persist the input.
export function usePageSpeedCredentials(open: boolean, desktop: boolean) {
  const [status, setStatus] = useState<CredentialStatus>();
  const [apiKey, setApiKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    if (!desktop || busy.current) return;
    const request = ++revision.current;
    setLoading(true); setError(undefined);
    try {
      const result = await invoke<CredentialStatus>("get_page_speed_credential_status");
      if (request === revision.current) setStatus(result);
    } catch (caught) { if (request === revision.current) setError(message(caught)); }
    finally { if (request === revision.current) setLoading(false); }
  }, [desktop]);
  useEffect(() => {
    if (open) void refresh();
    else { setApiKey(""); revision.current++; }
  }, [open, refresh]);
  const update = async (clear: boolean) => {
    if (!desktop || busy.current || (!clear && !apiKey.trim())) return;
    busy.current = true;
    revision.current++;
    setLoading(true); setError(undefined);
    try {
      setStatus(await invoke<CredentialStatus>(clear ? "clear_page_speed_api_key" : "save_page_speed_api_key",
        clear ? undefined : { request: { apiKey: apiKey.trim() } }));
      setApiKey("");
    } catch (caught) { setError(message(caught)); }
    finally { busy.current = false; setLoading(false); }
  };
  return { status, apiKey, setApiKey, loading, error, refresh, save: () => update(false), clear: () => update(true) };
}

export function PageSpeedSettings({ credentials, desktop }: {
  credentials: ReturnType<typeof usePageSpeedCredentials>; desktop: boolean;
}) {
  const { status, apiKey, setApiKey, loading, error, refresh, save, clear } = credentials;
  return <div className="page-speed-settings settings-grid compact">
    <h3 className="settings-wide">PageSpeed Insights</h3>
    <label className="settings-wide">API key <span className="detail-muted">(optional)</span>
      <input type="password" aria-label="PageSpeed API key" autoComplete="off" spellCheck={false}
        maxLength={512} value={apiKey} disabled={!desktop || loading}
        placeholder={status?.keySaved ? "Saved in OS credential store" : "Paste a Google API key"}
        onChange={(event) => setApiKey(event.target.value)} />
    </label>
    <div className="integration-status settings-wide" role="status">
      <span className={status?.keySaved ? "ok" : "muted"}>{loading ? "Checking credentials…" : status?.keySaved ? "API key saved" : "No saved API key"}</span>
      {status?.message ? <span className="danger">{status.message}</span> : null}
      {!desktop ? <span>Open the desktop app to manage credentials.</span> : null}
    </div>
    <div className="settings-actions settings-wide">
      <button className="settings-action-button primary" data-action="save-pagespeed-key"
        disabled={!desktop || loading || !apiKey.trim()} onClick={() => void save()}><Check size={15} />Save API key</button>
      <button className="settings-action-button danger" data-action="clear-pagespeed-key"
        disabled={!desktop || loading} onClick={() => void clear()}><Trash2 size={15} />Clear key</button>
      <button className="settings-action-button" disabled={!desktop || loading} onClick={() => void refresh()}><RefreshCw size={15} />Check status</button>
    </div>
    <p className="settings-help settings-wide">Save and Clear take effect immediately. Run measurements from the selected URL’s PageSpeed tab.</p>
    {error ? <p className="settings-validation-error settings-wide" role="alert">{error}</p> : null}
  </div>;
}

const metric = (value: number | null | undefined, unit = "") => value == null || !Number.isFinite(value) || value < 0
  ? "Not available" : `${value.toLocaleString(undefined, { maximumFractionDigits: 3 })}${unit}`;
const score = (value: number | null | undefined) => value == null || !Number.isFinite(value) || value < 0 || value > 1 ? "Not available" : String(Math.round(value * 100));

export type PageSpeedCategory = "performance" | "accessibility" | "bestPractices" | "seo";
export const pageSpeedCategoryLabels: [PageSpeedCategory, string][] = [["performance", "Performance"], ["accessibility", "Accessibility"], ["bestPractices", "Best practices"], ["seo", "SEO"]];

export function PageSpeedPanel({ snapshot, strategy, onStrategy, disabledReason, onRun, onConfigure, categories, onCategories, selectedCount, onRunSelected, bulkStatus }: {
  snapshot?: PageSpeedSnapshot | null; strategy: PageSpeedStrategy; onStrategy: (strategy: PageSpeedStrategy) => void;
  disabledReason?: string; onRun: () => void; onConfigure: () => void;
  categories: PageSpeedCategory[]; onCategories: (categories: PageSpeedCategory[]) => void;
  selectedCount: number; onRunSelected: () => void; bulkStatus?: string;
}) {
  const reportedTime = Date.parse(snapshot?.fetchedAt ?? "");
  const date = new Date(Number.isFinite(reportedTime) ? reportedTime : snapshot?.completedAtMs ?? NaN);
  return <div className="page-speed-panel" id="detail-panel-pagespeed" role="tabpanel" aria-labelledby="detail-tab-pagespeed">
    <div className="page-speed-toolbar">
      <label>Device <select aria-label="PageSpeed device" value={strategy} disabled={Boolean(disabledReason)}
        onChange={(event) => onStrategy(event.target.value as PageSpeedStrategy)}><option value="mobile">Mobile</option><option value="desktop">Desktop</option></select></label>
      <button className="primary" data-action="run-pagespeed" onClick={onRun} disabled={Boolean(disabledReason)} title={disabledReason}>
        <RefreshCw size={14} />{snapshot ? "Measure again" : "Run PageSpeed"}
      </button>
      <button data-action="run-pagespeed-selected" onClick={onRunSelected} disabled={Boolean(disabledReason) || selectedCount === 0}
        title={selectedCount === 0 ? "Select rows in the grid (Ctrl/Cmd-click) to measure several URLs" : disabledReason}>
        <RefreshCw size={14} />Measure {selectedCount > 0 ? `${selectedCount.toLocaleString()} selected` : "selected"}
      </button>
      <button data-action="configure-pagespeed" onClick={onConfigure} title="PageSpeed API settings"><Settings size={14} />API settings</button>
    </div>
    <div className="page-speed-categories" role="group" aria-label="Lighthouse categories">
      {pageSpeedCategoryLabels.map(([key, label]) => <label key={key}>
        <input type="checkbox" checked={categories.includes(key)} disabled={Boolean(disabledReason)}
          onChange={(event) => onCategories(event.target.checked ? [...categories, key] : categories.length > 1 ? categories.filter((item) => item !== key) : categories)} />
        {label}
      </label>)}
    </div>
    <p className="page-speed-caption">{bulkStatus ?? disabledReason ?? "Google runs this lab test remotely for the selected URL. Selected rows run one after another; quota limits are retried and rows already measured with this device are skipped."}</p>
    {snapshot ? <div className="page-speed-result">
      <div className="page-speed-attribution">
        <strong>{snapshot.strategy === "desktop" ? "Desktop" : "Mobile"} · Lab results</strong>
        {Number.isFinite(date.getTime()) ? <time dateTime={date.toISOString()}>{date.toLocaleString()}</time> : null}
        {snapshot.lighthouseVersion ? <span>Lighthouse {snapshot.lighthouseVersion}</span> : null}
      </div>
      <dl className="page-speed-scores">
        {([["Performance", snapshot.performanceScore], ["Accessibility", snapshot.accessibilityScore], ["Best practices", snapshot.bestPracticesScore], ["SEO", snapshot.seoScore]] as const)
          .map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{score(value)}</dd></div>)}
      </dl>
      <dl className="page-speed-metrics">
        <div><dt>Largest Contentful Paint</dt><dd>{metric(snapshot.lcpMs, " ms")}</dd></div>
        <div><dt>Total Blocking Time</dt><dd>{metric(snapshot.tbtMs, " ms")}</dd></div>
        <div><dt>Cumulative Layout Shift</dt><dd>{metric(snapshot.cls)}</dd></div>
      </dl>
      <dl className="page-speed-urls">
        <dt>Requested URL</dt><dd>{snapshot.requestedUrl}</dd>
        {snapshot.finalUrl ? <><dt>Final URL</dt><dd>{snapshot.finalUrl}</dd></> : null}
      </dl>
    </div> : <p className="page-speed-empty">No measurement yet.</p>}
  </div>;
}

// Real-user Core Web Vitals from the Chrome UX Report; separate from the Lighthouse lab run above.
export function FieldVitalsPanel({ snapshot, formFactor, onFormFactor, disabledReason, onRun }: {
  snapshot?: FieldVitalsSnapshot | null; formFactor: FieldFormFactor; onFormFactor: (value: FieldFormFactor) => void;
  disabledReason?: string; onRun: () => void;
}) {
  const date = new Date(snapshot?.completedAtMs ?? NaN);
  return <div className="field-vitals-panel">
    <div className="page-speed-toolbar">
      <label>Form factor <select aria-label="Field data form factor" value={formFactor} disabled={Boolean(disabledReason)}
        onChange={(event) => onFormFactor(event.target.value as FieldFormFactor)}>
        <option value="phone">Phone</option><option value="desktop">Desktop</option><option value="tablet">Tablet</option></select></label>
      <button className="primary" data-action="run-field-vitals" onClick={onRun} disabled={Boolean(disabledReason)} title={disabledReason}>
        <RefreshCw size={14} />{snapshot ? "Fetch field data again" : "Fetch field data"}
      </button>
    </div>
    <p className="page-speed-caption">{disabledReason ?? "Chrome UX Report p75 values from real Chrome users over the last 28 days."}</p>
    {snapshot ? <div className="page-speed-result">
      <div className="page-speed-attribution">
        <strong>{snapshot.formFactor === "desktop" ? "Desktop" : snapshot.formFactor === "tablet" ? "Tablet" : "Phone"} · Field data</strong>
        {Number.isFinite(date.getTime()) ? <time dateTime={date.toISOString()}>{date.toLocaleString()}</time> : null}
        {snapshot.collectionPeriodStart && snapshot.collectionPeriodEnd ? <span>{snapshot.collectionPeriodStart} to {snapshot.collectionPeriodEnd}</span> : null}
      </div>
      {snapshot.hasData ? <dl className="page-speed-metrics">
        <div><dt>Largest Contentful Paint (p75)</dt><dd>{metric(snapshot.lcpMsP75, " ms")}</dd></div>
        <div><dt>Interaction to Next Paint (p75)</dt><dd>{metric(snapshot.inpMsP75, " ms")}</dd></div>
        <div><dt>Cumulative Layout Shift (p75)</dt><dd>{metric(snapshot.clsP75)}</dd></div>
        <div><dt>First Contentful Paint (p75)</dt><dd>{metric(snapshot.fcpMsP75, " ms")}</dd></div>
        <div><dt>Time to First Byte (p75)</dt><dd>{metric(snapshot.ttfbMsP75, " ms")}</dd></div>
      </dl> : <p className="page-speed-empty">Chrome UX Report has no field data for this URL and form factor.</p>}
    </div> : <p className="page-speed-empty">No field data fetched yet.</p>}
  </div>;
}
