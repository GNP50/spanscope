import { Component, type ErrorInfo, type ReactNode } from 'react';

/** Keeps a failing view from blanking the whole report; resets when `resetKey` changes. */
export class ErrorBoundary extends Component<{ resetKey: string; children: ReactNode }, { error: Error | null; key: string }> {
  state = { error: null as Error | null, key: this.props.resetKey };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  static getDerivedStateFromProps(props: { resetKey: string }, state: { error: Error | null; key: string }) {
    return props.resetKey !== state.key ? { error: null, key: props.resetKey } : null;
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('spanscope view failed', error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="error-box" role="alert">
        This view could not render this profile: {this.state.error.message}. Other views remain available.
      </div>
    );
  }
}
