import { invoke } from "@tauri-apps/api/core";
import * as Dialog from "./Dialog";
import { ChevronLeft, ChevronRight, Download, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";

type Snippet = { url: string; title: string; description: string };
type Metrics = { titleLength: number; titlePixelWidth: number; descriptionLength: number; descriptionPixelWidth: number };
const blankSnippet = (): Snippet => ({ url: "https://example.com/", title: "", description: "" });

export default function SerpPreviewDialog({ open, onOpenChange, selected }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  selected?: Snippet;
}) {
  const [snippets, setSnippets] = useState<Snippet[]>(() => [{ ...(selected ?? blankSnippet()) }]);
  const [index, setIndex] = useState(0);
  const [mobile, setMobile] = useState(false);
  const [metrics, setMetrics] = useState<Metrics>();
  const [validationError, setValidationError] = useState<string>();
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const snippet = snippets[index];

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setMetrics(undefined);
    setValidationError(undefined);
    const timer = window.setTimeout(() => {
      void invoke<Metrics>("measure_serp_snippet", { snippet }).then((result) => {
        if (!cancelled) setMetrics(result);
      }).catch((caught) => {
        if (!cancelled) setValidationError(String(caught));
      });
    }, 120);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [open, snippet]);

  const edit = (change: Partial<Snippet>) => {
    setSnippets((rows) => rows.map((row, position) => position === index ? { ...row, ...change } : row));
    setMessage(undefined);
  };
  const importFile = async (file?: File) => {
    if (!file) return;
    setError(undefined); setMessage(undefined); setBusy(true);
    try {
      if (file.size > 5 * 1024 * 1024) throw new Error("Snippet CSV files must be at most 5 MiB.");
      const rows = await invoke<Snippet[]>("import_serp_snippets", { text: await file.text() });
      setSnippets(rows); setIndex(0);
      setMessage(`Imported ${rows.length.toLocaleString()} snippets from ${file.name}.`);
    } catch (caught) { setError(String(caught)); }
    finally { setBusy(false); }
  };
  const exportFile = async () => {
    setError(undefined); setMessage(undefined); setBusy(true);
    try {
      const result = await invoke<{ path: string; rowCount: number }>("export_serp_snippets", { snippets });
      setMessage(`Exported ${result.rowCount.toLocaleString()} snippets to ${result.path}.`);
    } catch (caught) { setError(String(caught)); }
    finally { setBusy(false); }
  };

  return <Dialog.Root open={open} onOpenChange={onOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay className="modal-backdrop" />
      <Dialog.Content className="serp-modal" inert={!open}>
        <div className="modal-header">
          <div>
            <Dialog.Title asChild><h2>SERP Preview</h2></Dialog.Title>
            <Dialog.Description className="settings-save-note">Edit local snippet drafts. Crawl results stay separate; export drafts to keep them after quitting.</Dialog.Description>
          </div>
          <Dialog.Close asChild><button title="Close SERP preview"><X size={16} /></button></Dialog.Close>
        </div>
        <div className="serp-content">
          <fieldset disabled={busy} className="serp-editor">
            <legend className="sr-only">Snippet editor</legend>
            <div className="serp-actions">
              <button onClick={() => edit(selected!)} disabled={!selected}>Use selected URL</button>
              <button onClick={() => { setSnippets((rows) => [...rows, blankSnippet()]); setIndex(snippets.length); }} disabled={snippets.length >= 1000}><Plus size={15} />New snippet</button>
              <button onClick={() => void exportFile()} disabled={!metrics || Boolean(validationError)}><Download size={15} />Export CSV</button>
            </div>
            <label>Import snippet CSV
              <input type="file" aria-label="Import snippet CSV" accept=".csv,text/csv" onChange={(event) => {
                const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; void importFile(file);
              }} />
            </label>
            <p className="settings-save-note">CSV: url, title, description (or meta_description). Import replaces these drafts, up to 1,000 rows / 5 MiB.</p>
            <div className="serp-navigation">
              <button aria-label="Previous snippet" disabled={index === 0} onClick={() => setIndex(index - 1)}><ChevronLeft size={16} /></button>
              <select aria-label="Snippet" value={index} onChange={(event) => setIndex(Number(event.target.value))}>
                {snippets.map((row, position) => <option key={position} value={position}>{position + 1}. {row.url}</option>)}
              </select>
              <button aria-label="Next snippet" disabled={index >= snippets.length - 1} onClick={() => setIndex(index + 1)}><ChevronRight size={16} /></button>
            </div>
            <label>URL<input aria-label="Snippet URL" type="url" value={snippet.url} maxLength={20000} onChange={(event) => edit({ url: event.target.value })} /></label>
            <label>Title<input aria-label="Snippet title" value={snippet.title} maxLength={20000} onChange={(event) => edit({ title: event.target.value })} /></label>
            <span className="serp-metrics">{metrics ? `${metrics.titleLength} characters · ${metrics.titlePixelWidth} estimated px` : "Measuring title…"}</span>
            <label>Description<textarea aria-label="Snippet description" rows={3} value={snippet.description} maxLength={20000} onChange={(event) => edit({ description: event.target.value })} /></label>
            <span className="serp-metrics">{metrics ? `${metrics.descriptionLength} characters · ${metrics.descriptionPixelWidth} estimated px` : "Measuring description…"}</span>
          </fieldset>
          {validationError || error ? <p role="alert" className="serp-error">{error ?? validationError}</p> : null}
          {message ? <p role="status" className="serp-message">{message}</p> : null}
          <div className="serp-actions" role="group" aria-label="Preview device">
            <button aria-pressed={!mobile} onClick={() => setMobile(false)}>Desktop</button>
            <button aria-pressed={mobile} onClick={() => setMobile(true)}>Mobile</button>
          </div>
          <section className={`serp-preview${mobile ? " mobile" : ""}`} aria-label="Search snippet preview">
            <p className="serp-url">{snippet.url}</p>
            <p className="serp-title">{snippet.title || "Untitled page"}</p>
            <p className="serp-description">{snippet.description || "No description provided."}</p>
          </section>
          <p className="settings-save-note">Illustrative preview; search engines can rewrite snippets. Pixel widths use the same estimate as the crawler.</p>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}
