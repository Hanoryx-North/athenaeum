import { type FormEvent, useState } from "react";

import type { KnowledgeNode, Vocabularies } from "../api";

/**
 * Offered while the vocabularies are still loading. Matches the library's own
 * default node type, so a node added before they arrive is stored as what the
 * CLI would have stored.
 */
const FALLBACK_NODE_TYPE = "observation";

export interface NodeListProps {
  nodes: KnowledgeNode[];
  selectedId: string | null;
  vocab: Vocabularies | null;
  busy: boolean;
  onSelect: (id: string) => void;
  onAdd: (type: string, title: string) => void;
}

/** The left pane: add a node, and pick one to look at. */
export function NodeList({ nodes, selectedId, vocab, busy, onSelect, onAdd }: NodeListProps) {
  const [title, setTitle] = useState("");
  const [type, setType] = useState(FALLBACK_NODE_TYPE);

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (trimmed === "") return;
    onAdd(type, trimmed);
    setTitle("");
  }

  return (
    <section className="pane list">
      <form className="add" onSubmit={submit}>
        <select
          value={type}
          onChange={(event) => setType(event.target.value)}
          aria-label="Node type"
        >
          {(vocab?.nodeTypes ?? [FALLBACK_NODE_TYPE]).map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          placeholder="New node title"
          aria-label="New node title"
        />
        <button type="submit" disabled={busy || title.trim() === ""}>
          Add
        </button>
      </form>

      {nodes.length === 0 ? (
        <p className="hint">Nothing here yet.</p>
      ) : (
        <ul>
          {nodes.map((node) => (
            <li key={node.id}>
              <button
                type="button"
                className={node.id === selectedId ? "row selected" : "row"}
                onClick={() => onSelect(node.id)}
                aria-current={node.id === selectedId}
              >
                <span className="type">{node.type}</span>
                <span className="title">{node.title}</span>
                {node.status !== "valid" && <span className="status">{node.status}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
