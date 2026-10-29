// 输入法直接上屏的文字（中文标点、数字等，不经候选框）由应用发给 shell。不碰 DOM。
//
// macOS 的 webview 先发 input、再发 keydown 229，xterm 的两条路（input 事件和
// keydown 229 之后比对 textarea）按事件先后会丢字或重复（xterm.js #6144）。
// 这里只走 input 一条路，keydown 229 不交给 xterm，所以每个字符只发一次。

export interface ImeKey {
  type: string;
  keyCode: number;
  isComposing: boolean;
}

export interface ImeInputEvent {
  data: string | null;
  inputType: string;
  isComposing: boolean;
}

export class ImeDirectInput {
  /** Set by compositionend until the next key event: that commit is xterm's. */
  private afterComposition = false;
  /** xterm typed this key from keypress (space, A–Z); its input event is a repeat. */
  private keyPressed = false;

  /** For xterm's custom key handler. True when xterm must not handle this keydown. */
  key(event: ImeKey): boolean {
    if (event.type === "keypress") {
      this.keyPressed = true;
      return false;
    }
    this.keyPressed = false;
    if (event.type === "keyup" || (event.type === "keydown" && !event.isComposing)) {
      this.afterComposition = false;
    }
    return event.type === "keydown" && event.keyCode === 229 && !event.isComposing;
  }

  compositionEnd(): void {
    this.afterComposition = true;
  }

  /** Text to send for an input event on xterm's textarea, or null to leave it to xterm. */
  input(event: ImeInputEvent): string | null {
    if (
      event.inputType !== "insertText" ||
      !event.data ||
      event.isComposing ||
      this.afterComposition ||
      this.keyPressed
    ) {
      return null;
    }
    return event.data;
  }
}
