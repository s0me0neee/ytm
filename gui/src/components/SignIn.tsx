import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AudioLines, LoaderCircle } from "lucide-react";
import { errorText } from "../format";

/** First run: borrow the YouTube Music session a local browser already has. */
export function SignIn({ browsers, onSignedIn }: { browsers: string[]; onSignedIn: () => void }) {
  const [browser, setBrowser] = useState(browsers[0] ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const chosen = browser || browsers[0] || "";

  async function signIn() {
    setBusy(true);
    setError("");
    try {
      await invoke("sign_in", { browser: chosen });
      onSignedIn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-1 items-center justify-center p-6">
      <div className="flex w-full max-w-sm flex-col items-center rounded-3xl p-8 text-center glass">
        <span className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl bg-accent text-white shadow-lg shadow-accent/30">
          <AudioLines size={30} />
        </span>
        <h1 className="text-title font-bold tracking-tight text-ink">Sign in to YouTube Music</h1>
        <p className="mt-2 text-body leading-relaxed text-ink-dim">
          ytm uses the session of a browser you're already signed in to. Pick it, and its YouTube cookies are copied over.
        </p>

        <label className="mt-6 w-full text-left">
          <span className="mb-1.5 block text-label font-semibold tracking-wider text-ink-faint uppercase">Browser</span>
          <select value={chosen} onChange={(e) => setBrowser(e.target.value)} disabled={busy} className="field w-full">
            {browsers.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>

        <button onClick={() => void signIn()} disabled={busy || !chosen} className="btn btn-primary mt-4 w-full justify-center py-2">
          {busy ? (
            <>
              <LoaderCircle size={14} className="animate-spin" /> Reading cookies…
            </>
          ) : (
            "Sign in"
          )}
        </button>

        {error && (
          <p role="alert" className="mt-4 w-full rounded-xl bg-accent/10 px-3 py-2 text-left text-meta text-accent-2 select-text">
            {error}
          </p>
        )}
        <p className="mt-5 text-label leading-relaxed text-ink-faint">
          The cookies stay on this machine in the app's config folder, readable only by you, and are shared with the terminal app.
        </p>
      </div>
    </div>
  );
}
