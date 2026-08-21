export interface EmptyStateProps {
  onOpen: () => void;
  busy: boolean;
}

/** What the window shows before any project has been opened. */
export function EmptyState({ onOpen, busy }: EmptyStateProps) {
  return (
    <div className="empty">
      <h1>Open a project</h1>
      <p>
        Athenaeum keeps its knowledge in an <code>.athenaeum/</code> directory beside your
        code. Pick a project folder and it will be created if it is not there yet.
      </p>
      <button type="button" onClick={onOpen} disabled={busy}>
        Choose folder…
      </button>
    </div>
  );
}
