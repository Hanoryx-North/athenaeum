import type { FormEvent } from "react";

export interface TopBarProps {
  query: string;
  hasWorkspace: boolean;
  busy: boolean;
  onQueryChange: (query: string) => void;
  onSearch: () => void;
  onBrief: () => void;
  onChooseProject: () => void;
}

/** Search, brief, and the project picker. */
export function TopBar({
  query,
  hasWorkspace,
  busy,
  onQueryChange,
  onSearch,
  onBrief,
  onChooseProject,
}: TopBarProps) {
  function submit(event: FormEvent) {
    event.preventDefault();
    onSearch();
  }

  return (
    <header className="topbar">
      <div className="brand">Athenaeum</div>
      <form className="search" onSubmit={submit}>
        <input
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={hasWorkspace ? "Search, or describe a task…" : "Open a project first"}
          disabled={!hasWorkspace}
          aria-label="Search knowledge"
        />
        <button type="submit" disabled={!hasWorkspace || busy}>
          Search
        </button>
        <button type="button" onClick={onBrief} disabled={!hasWorkspace || busy}>
          Brief
        </button>
      </form>
      <button type="button" className="ghost" onClick={onChooseProject} disabled={busy}>
        {hasWorkspace ? "Change project" : "Open project"}
      </button>
    </header>
  );
}
