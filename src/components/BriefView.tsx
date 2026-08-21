import type { KnowledgeBrief } from "../api";

export interface BriefViewProps {
  brief: KnowledgeBrief;
  onClose: () => void;
}

/** The brief an agent would receive for the current task. */
export function BriefView({ brief, onClose }: BriefViewProps) {
  return (
    <article className="brief">
      <header>
        <h2>Knowledge brief</h2>
        <button type="button" className="ghost" onClick={onClose}>
          Close
        </button>
      </header>
      <p className="task">{brief.task}</p>

      {brief.warnings.length > 0 && (
        <div className="banner warn">
          <strong>Warnings</strong>
          <ul>
            {brief.warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        </div>
      )}

      {brief.nodes.length === 0 ? (
        <p className="hint">Nothing relevant is known about this yet.</p>
      ) : (
        <ol className="brief-nodes">
          {brief.nodes.map((node) => (
            <li key={node.id}>
              <strong>{node.title}</strong> <span className="type">{node.type}</span>
              <div>{node.summary}</div>
            </li>
          ))}
        </ol>
      )}

      {brief.relationships.length > 0 && (
        <>
          <h3>Relationships</h3>
          <ul className="relations">
            {/* Keyed by the relationship itself: an edge is unique on
                (from, type, to), so this is stable identity rather than a
                position that shifts when the brief is reassembled. */}
            {brief.relationships.map((relation) => (
              <li key={`${relation.from}|${relation.type}|${relation.to}`}>
                {relation.from} → <span className="edge">{relation.type}</span> → {relation.to}
              </li>
            ))}
          </ul>
        </>
      )}

      {brief.anchors.length > 0 && (
        <>
          <h3>Source anchors</h3>
          <ul className="evidence">
            {brief.anchors.map((anchor) => (
              <li key={anchor}>
                <code>{anchor}</code>
              </li>
            ))}
          </ul>
        </>
      )}

      {brief.truncated && <p className="hint">More related knowledge exists beyond the budget.</p>}
    </article>
  );
}
