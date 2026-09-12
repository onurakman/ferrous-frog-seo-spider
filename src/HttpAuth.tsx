import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { Check, RefreshCw, Trash2 } from "lucide-react";

type HttpAuthStatus = { saved: boolean; username?: string | null; keyringAvailable: boolean; message?: string | null };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

// Basic credentials live in the OS credential store; the inputs are drafts that never persist.
export type CredentialCommands = { status: string; save: string; clear: string };
export const basicAuthCommands: CredentialCommands = { status: "get_http_auth_status", save: "save_http_auth_credentials", clear: "clear_http_auth_credentials" };
export const formLoginCommands: CredentialCommands = { status: "get_form_login_status", save: "save_form_login_credentials", clear: "clear_form_login_credentials" };

export function useHttpAuthCredentials(open: boolean, desktop: boolean, commands: CredentialCommands = basicAuthCommands) {
  const [status, setStatus] = useState<HttpAuthStatus>();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    if (!desktop || busy.current) return;
    const request = ++revision.current;
    setLoading(true); setError(undefined);
    try {
      const result = await invoke<HttpAuthStatus>(commands.status);
      if (request === revision.current) setStatus(result);
    } catch (caught) { if (request === revision.current) setError(message(caught)); }
    finally { if (request === revision.current) setLoading(false); }
  }, [desktop, commands.status]);
  useEffect(() => {
    if (open) void refresh();
    else { setUsername(""); setPassword(""); revision.current++; }
  }, [open, refresh]);
  const update = async (clear: boolean) => {
    if (!desktop || busy.current || (!clear && (!username.trim() || !password))) return;
    busy.current = true;
    revision.current++;
    setLoading(true); setError(undefined);
    try {
      setStatus(await invoke<HttpAuthStatus>(clear ? commands.clear : commands.save,
        clear ? undefined : { request: { username: username.trim(), password } }));
      setUsername(""); setPassword("");
    } catch (caught) { setError(message(caught)); }
    finally { busy.current = false; setLoading(false); }
  };
  return { status, username, setUsername, password, setPassword, loading, error, refresh, save: () => update(false), clear: () => update(true) };
}

export function HttpAuthSettings({ credentials, desktop, title = "HTTP authentication", prefix = "HTTP auth", action = "http-auth", help }: {
  credentials: ReturnType<typeof useHttpAuthCredentials>; desktop: boolean; title?: string; prefix?: string; action?: string; help?: string;
}) {
  const { status, username, setUsername, password, setPassword, loading, error, refresh, save, clear } = credentials;
  return <div className={`http-auth-settings settings-grid compact ${action}-settings`}>
    <h3 className="settings-wide">{title}</h3>
    <label>Username
      <input aria-label={`${prefix} username`} autoComplete="off" spellCheck={false} maxLength={512} value={username}
        disabled={!desktop || loading} placeholder={status?.saved ? status.username ?? "" : "User name"}
        onChange={(event) => setUsername(event.target.value)} />
    </label>
    <label>Password
      <input type="password" aria-label={`${prefix} password`} autoComplete="off" spellCheck={false} maxLength={1024} value={password}
        disabled={!desktop || loading} placeholder={status?.saved ? "Saved in OS credential store" : "Password"}
        onChange={(event) => setPassword(event.target.value)} />
    </label>
    <div className="integration-status settings-wide" role="status">
      <span className={status?.saved ? "ok" : "muted"}>{loading ? "Checking credentials…" : status?.saved ? `Credentials saved for ${status.username ?? "user"}` : "No saved credentials"}</span>
      {status?.message ? <span className="danger">{status.message}</span> : null}
      {!desktop ? <span>Open the desktop app to manage credentials.</span> : null}
    </div>
    <div className="settings-actions settings-wide">
      <button className="settings-action-button primary" data-action={`save-${action}`}
        disabled={!desktop || loading || !username.trim() || !password} onClick={() => void save()}><Check size={15} />Save credentials</button>
      <button className="settings-action-button danger" data-action={`clear-${action}`}
        disabled={!desktop || loading} onClick={() => void clear()}><Trash2 size={15} />Clear credentials</button>
      <button className="settings-action-button" disabled={!desktop || loading} onClick={() => void refresh()}><RefreshCw size={15} />Check status</button>
    </div>
    <p className="settings-help settings-wide">{help ?? "Save and Clear take effect immediately and stay outside profiles. Credentials are sent only to the starting origin (including robots.txt, sitemaps and redirects there) as Basic authentication, or in reply to a Digest challenge; browser rendering does not use them."}</p>
    {error ? <p className="settings-validation-error settings-wide" role="alert">{error}</p> : null}
  </div>;
}
