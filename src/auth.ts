import { invoke } from "@tauri-apps/api/core";

export type AuthStateName = "needsSetup" | "locked" | "unlocked" | "unavailable";

export interface AuthSnapshot {
  state: AuthStateName;
  retryAfterMs: number;
  message?: string;
}

export function authStatus(): Promise<AuthSnapshot> {
  return invoke<AuthSnapshot>("auth_status");
}

export function authSetup(
  username: string,
  secret: string,
  confirm: string,
): Promise<AuthSnapshot> {
  return invoke<AuthSnapshot>("auth_setup", { username, secret, confirm });
}

export function authLogin(username: string, secret: string): Promise<AuthSnapshot> {
  return invoke<AuthSnapshot>("auth_login", { username, secret });
}
