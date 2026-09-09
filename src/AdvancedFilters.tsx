import { invoke } from "@tauri-apps/api/core";
import * as Dialog from "./Dialog";
import { ListFilter, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

type Field = "url" | "finalUrl" | "title" | "metaDescription" | "canonical" | "statusCode" | "depth" | "wordCount" | "responseTimeMs" | "indexability";
type Operator = "contains" | "notContains" | "equals" | "notEquals" | "isEmpty" | "isNotEmpty" | "lessThan" | "greaterThan";
type Rule = { field: Field; operator: Operator; value: string };
export type GridFilterGroup = { match: "all" | "any"; rules: Rule[] };

const fields: Array<{ value: Field; label: string; numeric?: boolean }> = [
  { value: "url", label: "URL" },
  { value: "finalUrl", label: "Final URL" },
  { value: "title", label: "Title" },
  { value: "metaDescription", label: "Meta description" },
  { value: "canonical", label: "Canonical URL" },
  { value: "statusCode", label: "Status code", numeric: true },
  { value: "depth", label: "Crawl depth", numeric: true },
  { value: "wordCount", label: "Word count", numeric: true },
  { value: "responseTimeMs", label: "Response time (ms)", numeric: true },
  { value: "indexability", label: "Indexability" },
];
const textOperators: Operator[] = ["contains", "notContains", "equals", "notEquals", "isEmpty", "isNotEmpty"];
const numberOperators: Operator[] = ["equals", "notEquals", "lessThan", "greaterThan"];
const operatorLabels: Record<Operator, string> = {
  contains: "Contains", notContains: "Does not contain", equals: "Equals", notEquals: "Does not equal",
  isEmpty: "Is empty", isNotEmpty: "Is not empty", lessThan: "Less than", greaterThan: "Greater than",
};

export default function AdvancedFilters({ value, onApply, disabled }: {
  value?: GridFilterGroup;
  onApply: (value: GridFilterGroup | undefined) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<GridFilterGroup>({ match: "all", rules: [] });
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string>();
  const revision = useRef(0);
  useEffect(() => () => { revision.current++; }, []);
  const changeOpen = (next: boolean) => {
    revision.current++;
    setApplying(false);
    if (next) { setError(undefined); setDraft(value ?? { match: "all", rules: [] }); }
    setOpen(next);
  };
  const edit = (index: number, patch: Partial<Rule>) => {
    setError(undefined);
    setDraft((previous) => ({ ...previous, rules: previous.rules.map((rule, at) => at === index ? { ...rule, ...patch } : rule) }));
  };
  const apply = async () => {
    if (applying) return;
    const request = ++revision.current;
    setApplying(true);
    setError(undefined);
    try {
      await invoke("validate_result_filters", { filters: draft });
      if (request !== revision.current) return;
      onApply(draft.rules.length ? draft : undefined);
      changeOpen(false);
    } catch (caught) {
      if (request === revision.current) setError(String(caught));
    } finally {
      if (request === revision.current) setApplying(false);
    }
  };

  return <Dialog.Root open={open} onOpenChange={changeOpen}>
    <Dialog.Trigger asChild><button aria-label="Advanced filters" title="Advanced filters" aria-pressed={Boolean(value?.rules.length)} disabled={disabled}>
      <ListFilter size={15} />{value?.rules.length ? value.rules.length : null}
    </button></Dialog.Trigger>
    <Dialog.Portal>
      <Dialog.Overlay className="modal-backdrop" />
      <Dialog.Content className="advanced-filters-modal" inert={!open}>
        <div className="modal-header">
          <div><Dialog.Title>Advanced filters</Dialog.Title><Dialog.Description>Combine conditions with the current audit, search and segment.</Dialog.Description></div>
          <Dialog.Close asChild><button aria-label="Close advanced filters"><X size={18} /></button></Dialog.Close>
        </div>
        <form onSubmit={(event) => { event.preventDefault(); void apply(); }}>
          <fieldset disabled={applying} className="filter-editor">
            <legend className="sr-only">Filter conditions</legend>
            <label className="filter-match">Match
              <select aria-label="Match conditions" value={draft.match} onChange={(event) => setDraft({ ...draft, match: event.target.value as GridFilterGroup["match"] })}>
                <option value="all">All conditions</option><option value="any">Any condition</option>
              </select>
            </label>
            <div className="filter-rules">
              {draft.rules.map((rule, index) => {
                const numeric = fields.find((field) => field.value === rule.field)?.numeric;
                const needsValue = rule.operator !== "isEmpty" && rule.operator !== "isNotEmpty";
                return <div className="filter-rule" key={index}>
                  <label>Field<select aria-label={`Filter field ${index + 1}`} value={rule.field} onChange={(event) => {
                    const field = event.target.value as Field;
                    edit(index, { field, operator: fields.find((item) => item.value === field)?.numeric ? "equals" : "contains", value: "" });
                  }}>{fields.map((field) => <option key={field.value} value={field.value}>{field.label}</option>)}</select></label>
                  <label>Condition<select aria-label={`Filter operator ${index + 1}`} value={rule.operator} onChange={(event) => {
                    const operator = event.target.value as Operator;
                    edit(index, { operator, ...(operator === "isEmpty" || operator === "isNotEmpty" ? { value: "" } : {}) });
                  }}>{(numeric ? numberOperators : textOperators).map((operator) => <option key={operator} value={operator}>{operatorLabels[operator]}</option>)}</select></label>
                  {needsValue ? <label>Value<input aria-label={`Filter value ${index + 1}`} type={numeric ? "number" : "text"} step="any" required={numeric} maxLength={2000}
                    value={rule.value} onChange={(event) => edit(index, { value: event.target.value })} /></label> : <span />}
                  <button type="button" aria-label={`Remove filter condition ${index + 1}`} title="Remove condition" onClick={() => {
                    setError(undefined); setDraft({ ...draft, rules: draft.rules.filter((_, at) => at !== index) });
                  }}><X size={16} /></button>
                </div>;
              })}
            </div>
            <button type="button" aria-label="Add filter condition" disabled={draft.rules.length >= 20} onClick={() => setDraft({ ...draft, rules: [...draft.rules, { field: "url", operator: "contains", value: "" }] })}><Plus size={15} />Add condition</button>
            <p className="settings-save-note">Text ignores case and normalizes whitespace. Response times use milliseconds. Up to 20 conditions; removing all clears this filter.</p>
          </fieldset>
          {error ? <p role="alert" className="filter-error">{error}</p> : null}
          <div className="modal-actions">
            <Dialog.Close asChild><button type="button" aria-label="Cancel advanced filters">Cancel</button></Dialog.Close>
            <button type="submit" disabled={applying} className="primary">{applying ? "Applying…" : "Apply filters"}</button>
          </div>
        </form>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}
