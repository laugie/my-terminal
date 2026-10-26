// 终端配色和字体。列表与后端 config::TERMINAL_THEMES / TERMINAL_FONTS 相同，第一项是默认值。

export const ANSI_KEYS = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

type AnsiKey = (typeof ANSI_KEYS)[number];

/** The xterm `ITheme` fields every theme sets. */
export type TerminalPalette = Record<AnsiKey | "background" | "foreground" | "cursor" | "cursorAccent", string> & {
  selectionBackground: string;
};

export interface ThemeChoice {
  id: string;
  label: string;
  light: boolean;
  /** Passed to xterm. A light theme raises it so yellow or white text stays legible. */
  minimumContrastRatio?: number;
  palette: TerminalPalette;
}

export interface FontChoice {
  id: string;
  label: string;
  /** First family in the stack. Empty for the system default. */
  family: string;
  stack: string;
  /** Shipped with the app (SIL OFL 1.1). Others are used when installed. */
  bundled: boolean;
}

export const THEMES: readonly ThemeChoice[] = [
  {
    id: "midnight",
    label: "夜幕（默认）",
    light: false,
    palette: {
      background: "#0b0e13",
      foreground: "#d5dde8",
      cursor: "#8ab4ff",
      cursorAccent: "#0b0e13",
      selectionBackground: "rgba(138, 180, 255, 0.28)",
      black: "#1e2530",
      red: "#f2777a",
      green: "#8fd18a",
      yellow: "#e6c07b",
      blue: "#6ca6ff",
      magenta: "#c99cf0",
      cyan: "#63d3d8",
      white: "#c8d1dc",
      brightBlack: "#6b7686",
      brightRed: "#ff9a9d",
      brightGreen: "#addfa8",
      brightYellow: "#f2d49b",
      brightBlue: "#96c0ff",
      brightMagenta: "#dcbcf7",
      brightCyan: "#8ee3e7",
      brightWhite: "#f4f7fb",
    },
  },
  {
    id: "deep-sea",
    label: "深海",
    light: false,
    palette: {
      background: "#0f1722",
      foreground: "#cdd9e5",
      cursor: "#3ccfcf",
      cursorAccent: "#0f1722",
      selectionBackground: "rgba(60, 207, 207, 0.25)",
      black: "#1b2533",
      red: "#ff6b7a",
      green: "#4ade80",
      yellow: "#fbbf54",
      blue: "#4da3ff",
      magenta: "#c084fc",
      cyan: "#2dd4bf",
      white: "#cbd5e1",
      brightBlack: "#64748b",
      brightRed: "#ff8f9b",
      brightGreen: "#86efac",
      brightYellow: "#fcd34d",
      brightBlue: "#7cbcff",
      brightMagenta: "#d8b4fe",
      brightCyan: "#5eead4",
      brightWhite: "#f8fafc",
    },
  },
  {
    id: "one-dark",
    label: "One Dark",
    light: false,
    palette: {
      background: "#282c34",
      foreground: "#abb2bf",
      cursor: "#528bff",
      cursorAccent: "#282c34",
      selectionBackground: "rgba(103, 118, 150, 0.4)",
      black: "#3f4451",
      red: "#e05561",
      green: "#8cc265",
      yellow: "#d18f52",
      blue: "#4aa5f0",
      magenta: "#c162de",
      cyan: "#42b3c2",
      white: "#d7dae0",
      brightBlack: "#687080",
      brightRed: "#ff616e",
      brightGreen: "#a5e075",
      brightYellow: "#f0a45d",
      brightBlue: "#4dc4ff",
      brightMagenta: "#de73ff",
      brightCyan: "#4cd1e0",
      brightWhite: "#e6e6e6",
    },
  },
  {
    id: "dracula",
    label: "Dracula",
    light: false,
    palette: {
      background: "#282a36",
      foreground: "#f8f8f2",
      cursor: "#f8f8f2",
      cursorAccent: "#282a36",
      selectionBackground: "#44475a",
      black: "#21222c",
      red: "#ff5555",
      green: "#50fa7b",
      yellow: "#f1fa8c",
      blue: "#bd93f9",
      magenta: "#ff79c6",
      cyan: "#8be9fd",
      white: "#f8f8f2",
      brightBlack: "#6272a4",
      brightRed: "#ff6e6e",
      brightGreen: "#69ff94",
      brightYellow: "#ffffa5",
      brightBlue: "#d6acff",
      brightMagenta: "#ff92df",
      brightCyan: "#a4ffff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "tokyo-night",
    label: "Tokyo Night",
    light: false,
    palette: {
      background: "#1a1b26",
      foreground: "#c0caf5",
      cursor: "#c0caf5",
      cursorAccent: "#1a1b26",
      selectionBackground: "rgba(51, 70, 124, 0.7)",
      black: "#15161e",
      red: "#f7768e",
      green: "#9ece6a",
      yellow: "#e0af68",
      blue: "#7aa2f7",
      magenta: "#bb9af7",
      cyan: "#7dcfff",
      white: "#a9b1d6",
      brightBlack: "#565f89",
      brightRed: "#ff899d",
      brightGreen: "#9fe044",
      brightYellow: "#faba4a",
      brightBlue: "#8db0ff",
      brightMagenta: "#c7a9ff",
      brightCyan: "#a4daff",
      brightWhite: "#c0caf5",
    },
  },
  {
    id: "nord",
    label: "Nord",
    light: false,
    palette: {
      background: "#2e3440",
      foreground: "#d8dee9",
      cursor: "#d8dee9",
      cursorAccent: "#2e3440",
      selectionBackground: "rgba(76, 86, 106, 0.8)",
      black: "#3b4252",
      red: "#bf616a",
      green: "#a3be8c",
      yellow: "#ebcb8b",
      blue: "#81a1c1",
      magenta: "#b48ead",
      cyan: "#88c0d0",
      white: "#e5e9f0",
      brightBlack: "#616e88",
      brightRed: "#d06f79",
      brightGreen: "#b1d196",
      brightYellow: "#f0d399",
      brightBlue: "#8cafd2",
      brightMagenta: "#c895bf",
      brightCyan: "#8fbcbb",
      brightWhite: "#eceff4",
    },
  },
  {
    id: "solarized-dark",
    label: "Solarized Dark",
    light: false,
    palette: {
      background: "#002b36",
      foreground: "#93a1a1",
      cursor: "#93a1a1",
      cursorAccent: "#002b36",
      selectionBackground: "#073642",
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#eee8d5",
      brightBlack: "#586e75",
      brightRed: "#cb4b16",
      brightGreen: "#9fb33b",
      brightYellow: "#d4a72c",
      brightBlue: "#4aa3e8",
      brightMagenta: "#6c71c4",
      brightCyan: "#3fc0b3",
      brightWhite: "#fdf6e3",
    },
  },
  {
    id: "paper",
    label: "纸白（浅色）",
    light: true,
    minimumContrastRatio: 4.5,
    palette: {
      background: "#fafaf7",
      foreground: "#2b3036",
      cursor: "#2f6fd6",
      cursorAccent: "#fafaf7",
      selectionBackground: "rgba(47, 111, 214, 0.22)",
      black: "#2b3036",
      red: "#c0362c",
      green: "#2c7d3a",
      yellow: "#946200",
      blue: "#2f6fd6",
      magenta: "#9442b5",
      cyan: "#11808c",
      white: "#c9cdd2",
      brightBlack: "#6a717a",
      brightRed: "#d9493e",
      brightGreen: "#3a9a4a",
      brightYellow: "#a87400",
      brightBlue: "#4a86e8",
      brightMagenta: "#a95cc9",
      brightCyan: "#1a95a3",
      brightWhite: "#ffffff",
    },
  },
];

/** CJK falls back per glyph; the generic family comes last. */
const FALLBACK =
  '"SF Mono", Menlo, Monaco, "Cascadia Mono", Consolas, "PingFang SC", "Microsoft YaHei", ui-monospace, monospace';

function font(id: string, label: string, family: string, bundled = false): FontChoice {
  return { id, label, family, stack: `"${family}", ${FALLBACK}`, bundled };
}

export const FONTS: readonly FontChoice[] = [
  font("jetbrains-mono", "JetBrains Mono（内置）", "JetBrains Mono", true),
  { id: "system", label: "系统默认", family: "", stack: FALLBACK, bundled: false },
  font("sf-mono", "SF Mono", "SF Mono"),
  font("menlo", "Menlo", "Menlo"),
  font("cascadia-mono", "Cascadia Mono", "Cascadia Mono"),
  font("consolas", "Consolas", "Consolas"),
  font("fira-code", "Fira Code", "Fira Code"),
  font("meslo-nerd", "MesloLGS NF", "MesloLGS NF"),
];

export const DEFAULT_THEME = "midnight";
export const DEFAULT_FONT = "jetbrains-mono";

export function themeOf(id: unknown): ThemeChoice {
  return THEMES.find((theme) => theme.id === id) ?? (THEMES[0] as ThemeChoice);
}

export function fontOf(id: unknown): FontChoice {
  return FONTS.find((choice) => choice.id === id) ?? (FONTS[0] as FontChoice);
}

/** WCAG contrast ratio of two `#rrggbb` colors, 1 to 21. */
export function contrastRatio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((start) => {
    const value = Number.parseInt(hex.slice(start, start + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  const [r = 0, g = 0, b = 0] = channels;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export interface AppearanceView {
  theme: string;
  font: string;
  readOnly: boolean;
  message?: string | null;
}

export interface AppearanceChange {
  theme?: string;
  font?: string;
}

/**
 * The current theme and font for the terminal and the settings rows. Like the
 * zoom controller, a change applies at once and saves are queued in order, so
 * an older reply never moves the choice back.
 */
export class AppearanceController {
  theme = DEFAULT_THEME;
  font = DEFAULT_FONT;
  readOnly = false;
  message = "";
  private chain: Promise<void> = Promise.resolve();

  constructor(
    private readonly deps: {
      save(change: AppearanceChange): Promise<AppearanceView>;
      onChange?(): void;
    },
  ) {}

  load(view: AppearanceView): void {
    this.theme = themeOf(view.theme).id;
    this.font = fontOf(view.font).id;
    this.readOnly = view.readOnly;
    this.message = view.message ?? "";
    this.deps.onChange?.();
  }

  set(change: AppearanceChange): Promise<void> {
    const next: AppearanceChange = {};
    if (change.theme !== undefined && THEMES.some((theme) => theme.id === change.theme) && change.theme !== this.theme) {
      next.theme = change.theme;
    }
    if (change.font !== undefined && FONTS.some((choice) => choice.id === change.font) && change.font !== this.font) {
      next.font = change.font;
    }
    if (next.theme === undefined && next.font === undefined) {
      return this.chain;
    }
    this.theme = next.theme ?? this.theme;
    this.font = next.font ?? this.font;
    this.deps.onChange?.();
    this.chain = this.chain.then(async () => {
      try {
        const view = await this.deps.save(next);
        this.readOnly = view.readOnly;
        this.message = view.message ?? "";
      } catch (err) {
        this.message = err instanceof Error ? err.message : String(err);
      }
      this.deps.onChange?.();
    });
    return this.chain;
  }
}
