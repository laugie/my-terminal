// 关闭窗口或退出应用前的确认。后端拦下关闭请求后发来 close-requested，
// 这里用 #31 的应用内确认框询问（macOS 的 webview 不弹 window.confirm）。

export function quitMessage(sessions: number): string {
  return `退出 my-terminal？${sessions} 个会话会断开。`;
}

/**
 * One dialog at a time. Every request is reported back as shown, so the
 * backend can tell a live page from a hung one.
 */
export class QuitPrompt {
  private asking: Promise<void> | null = null;

  constructor(
    private readonly deps: {
      /** Tell the backend the request reached the page. */
      shown(): Promise<void>;
      ask(message: string): Promise<boolean>;
      /** Quit for real. */
      confirm(): Promise<void>;
    },
  ) {}

  request(sessions: number): Promise<void> {
    void this.deps.shown().catch(() => {});
    if (this.asking) {
      return this.asking;
    }
    this.asking = (async () => {
      try {
        if (await this.deps.ask(quitMessage(sessions))) {
          await this.deps.confirm();
        }
      } finally {
        this.asking = null;
      }
    })();
    return this.asking;
  }
}
