import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { errorText } from "./format";

/** Runs a command, toasting its failure; resolves `undefined` when it failed. */
export async function call<T = void>(cmd: string, args?: Record<string, unknown>, done?: string): Promise<T | undefined> {
  try {
    const out = await invoke<T>(cmd, args);
    if (done) toast(done);
    return out;
  } catch (e) {
    toast.error(errorText(e));
    return undefined;
  }
}

/** Fire-and-forget for transport keys, where a failure is still worth a toast. */
export function send(cmd: string, args?: Record<string, unknown>): void {
  void call(cmd, args);
}
