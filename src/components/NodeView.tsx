import type { NodeDetail } from "../api";

export interface NodeViewProps {
  detail: NodeDetail;
  onSelect: (id: string) => void;
}

/** One node with its relationships, evidence and history. */
export function NodeView({ detail, onSelect }: NodeViewProps) {
  const { node, relationships, evidence, history, utility } = detail;

  return (
    <article className="node">
      <h2>{node.title}</h2>
      <dl className="meta">
        <div><dt>type</dt><dd>{node.type}</dd></div>
        <div><dt>status</dt><dd>{node.status}</dd></div>
        <div><dt>authority</dt><dd>{node.authority}</dd></div>
        <div><dt>scope</dt><dd>{node.scope}</dd></div>
        <div><dt>version</dt><dd>{node.version}</dd></div>
        <div><dt>source</dt><dd>{node.provenance.source}</dd></div>
      </dl>

      {node.summary && <p className="summary">{node.summary}</p>}
      {node.content && <pre className="content">{node.content}</pre>}

      {relationships.length > 0 && (
        <>
          <h3>Relationships</h3>
          <ul className="relations">
            {relationships.map((relation) => (
              <li key={relation.edgeId}>
                <span className="edge">{relation.edgeType}</span>
                <button type="button" className="link" onClick={() => onSelect(relation.id)}>
                  {relation.title}
                </button>
                {/* Proposed edges are shown but marked, never silently
                    presented as established knowledge. */}
                {!relation.established && <span className="proposed">proposed</span>}
              </li>
            ))}
          </ul>
        </>
      )}

      {evidence.length > 0 && (
        <>
          <h3>Evidence</h3>
          <ul className="evidence">
            {evidence.map((item) => (
              <li key={item.id}>
                <code>
                  {item.anchor
                    ? `${item.anchor.file}${item.anchor.symbol ? `:${item.anchor.symbol}` : ""}`
                    : (item.note ?? item.kind)}
                </code>
                <span className="kind">{item.kind}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {history.length > 0 && (
        <>
          <h3>History</h3>
          <ul className="history">
            {history.map((version) => (
              <li key={version.version}>
                v{version.version} · {new Date(version.recordedAt).toLocaleString()}
                {version.reason ? ` · ${version.reason}` : ""}
              </li>
            ))}
          </ul>
        </>
      )}

      {utility.retrievals > 0 && (
        <p className="utility">
          Retrieved {utility.retrievals}×, used {utility.uses}×
        </p>
      )}
    </article>
  );
}
