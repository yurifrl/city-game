// Command palette: Ctrl/Cmd+K toggles a modal list of commands, typed text
// filters, arrows move the selection, Enter or click runs. The palette is
// presentation-only — commands are supplied by the composition root, keeping
// every side effect (network, replica) in main.ts.

export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
  run(): void;
}

export class CommandPalette {
  private commands: PaletteCommand[];
  private filtered: PaletteCommand[] = [];
  private active = 0;
  private host: HTMLDivElement | null = null;

  constructor(commands: PaletteCommand[]) {
    this.commands = commands;
    window.addEventListener("keydown", this.onKeydown);
  }

  dispose(): void {
    window.removeEventListener("keydown", this.onKeydown);
    this.close();
  }

  private onKeydown = (ev: KeyboardEvent): void => {
    if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "k") {
      ev.preventDefault();
      if (this.host === null) this.open();
      else this.close();
      return;
    }
    if (this.host === null) return;
    if (ev.key === "Escape") {
      ev.preventDefault();
      this.close();
    }
  };

  private open(): void {
    const host = document.createElement("div");
    host.id = "palette";
    const panel = document.createElement("div");
    panel.className = "palette-panel";
    const input = document.createElement("input");
    input.className = "palette-input";
    input.placeholder = "Type a command…";
    const list = document.createElement("ul");
    list.className = "palette-list";
    panel.append(input, list);
    host.append(panel);
    document.body.append(host);

    // Clicks on the dimmed backdrop close; clicks inside run the entry.
    host.addEventListener("pointerdown", (ev) => {
      if (ev.target === host) this.close();
    });

    input.addEventListener("input", () => {
      const needle = input.value.trim().toLowerCase();
      this.filtered = this.commands.filter((cmd) => cmd.label.toLowerCase().includes(needle));
      this.active = 0;
      this.render();
    });
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
        ev.preventDefault();
        if (this.filtered.length > 0) {
          const step = ev.key === "ArrowDown" ? 1 : -1;
          this.active = (this.active + step + this.filtered.length) % this.filtered.length;
          this.render();
        }
        return;
      }
      if (ev.key === "Enter") {
        ev.preventDefault();
        const cmd = this.filtered[this.active];
        if (cmd) this.choose(cmd);
      }
    });

    this.host = host;
    this.filtered = this.commands;
    this.active = 0;
    this.render();
    input.focus();
  }

  private close(): void {
    this.host?.remove();
    this.host = null;
  }

  private choose(cmd: PaletteCommand): void {
    this.close();
    cmd.run();
  }

  private render(): void {
    const list = this.host?.querySelector(".palette-list");
    if (!list) return;
    list.replaceChildren(
      ...this.filtered.map((cmd, i) => {
        const li = document.createElement("li");
        li.className = i === this.active ? "palette-item active" : "palette-item";
        const label = document.createElement("span");
        label.textContent = cmd.label;
        li.append(label);
        if (cmd.hint) {
          const hint = document.createElement("span");
          hint.className = "palette-hint";
          hint.textContent = cmd.hint;
          li.append(hint);
        }
        li.addEventListener("pointerdown", (ev) => {
          ev.preventDefault(); // keep focus in the input
          this.choose(cmd);
        });
        li.addEventListener("pointermove", () => {
          if (this.active !== i) {
            this.active = i;
            this.render();
          }
        });
        return li;
      }),
    );
  }
}
