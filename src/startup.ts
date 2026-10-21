export type StartupAuthState = "needsSetup" | "locked" | "unlocked" | "unavailable";

export type StartupDestination = "home" | "login" | "setup" | "unavailable";

export type LockStep = "setup" | "save" | "verify";

export type LockEffect = "nextLaunch" | "now";

export interface LockPlan {
  step: LockStep;
  effect: LockEffect;
}

/** Where to go at startup. A disabled lock opens the home page for every auth state. */
export function startupRoute(input: {
  lockEnabled: boolean;
  authState: StartupAuthState;
}): StartupDestination {
  if (!input.lockEnabled) {
    return "home";
  }
  switch (input.authState) {
    case "needsSetup":
      return "setup";
    case "locked":
      return "login";
    case "unlocked":
      return "home";
    case "unavailable":
      return "unavailable";
  }
}

export function toggleLockPlan(input: {
  from: boolean;
  to: boolean;
  hasPassword: boolean;
}): LockPlan {
  if (!input.from && input.to) {
    if (!input.hasPassword) {
      return { step: "setup", effect: "nextLaunch" };
    }
    return { step: "save", effect: "nextLaunch" };
  }
  if (input.from && !input.to) {
    if (input.hasPassword) {
      return { step: "verify", effect: "now" };
    }
    return { step: "save", effect: "now" };
  }
  return { step: "save", effect: "now" };
}

/** Same plan as `toggleLockPlan`, using the app's stored-secret flag. */
export function planLockChange(from: boolean, to: boolean, hasSecret: boolean): LockPlan {
  return toggleLockPlan({ from, to, hasPassword: hasSecret });
}
