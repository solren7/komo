import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  error: Error | null;
}

/** Last resort: a render error in one panel shouldn't leave a blank window with
 *  no way to find out why. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("komo renderer crashed", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="grid h-screen w-screen place-items-center bg-background p-6 text-foreground">
        <div className="flex max-w-lg flex-col gap-2">
          <div className="text-lg font-semibold">界面出错了</div>
          <pre className="overflow-auto rounded-lg border border-border bg-muted p-3 text-xs whitespace-pre-wrap">
            {error.message}
          </pre>
          <div className="text-sm text-muted-foreground">重新加载窗口可恢复。</div>
        </div>
      </div>
    );
  }
}
