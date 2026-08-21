import type { Stats } from "../api";

export interface StatusBarProps {
  stats: Stats | null;
  workspace: string | null;
}

/** Counts and the open project path. */
export function StatusBar({ stats, workspace }: StatusBarProps) {
  return (
    <footer className="statusbar">
      {stats ? (
        <span>
          {stats.liveNodes} live of {stats.nodes} nodes · {stats.edges} edges ·{" "}
          {stats.evidence} evidence · schema v{stats.schemaVersion}
        </span>
      ) : (
        <span>No project open</span>
      )}
      {workspace && <span className="path">{workspace}</span>}
    </footer>
  );
}
