import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import {
  api,
  errorMessage,
  type KnowledgeBrief,
  type KnowledgeNode,
  type NodeDetail,
  type Stats,
  type Vocabularies,
} from "./api";

/**
 * The application shell.
 *
 * Deliberately not a graph visualization (buildplan §33): the graph has to
 * hold something worth looking at before drawing it is the useful work. What
 * this gives you is the part that matters first — read, write, search,
 * navigate, and see the brief an agent would receive.
 */
export default function App() {
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [vocab, setVocab] = useState<Vocabularies | null>(null);
  const [nodes, setNodes] = useState<KnowledgeNode[]>([]);
  const [selected, setSelected] = useState<NodeDetail | null>(null);
  const [query, setQuery] = useState("");
  const [brief, setBrief] = useState<KnowledgeBrief | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [nextStats, nextNodes] = await Promise.all([api.stats(), api.listNodes()]);
      setStats(nextStats);
      setNodes(nextNodes);
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  // Reopen the last project on launch, so the app does not greet a returning
  // user with an empty screen and a file picker.
  useEffect(() => {
    void (async () => {
      try {
        const remembered = localStorage.getItem("athenaeum.workspace");
        if (remembered) {
          await api.openWorkspace(remembered);
          setWorkspace(await api.workspacePath());
          await refresh();
        }
        setVocab(await api.vocabularies());
      } catch (err) {
        setError(errorMessage(err));
      }
    })();
  }, [refresh]);

  async function chooseProject() {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked !== "string") return;
      setBusy(true);
      await api.openWorkspace(picked);
      localStorage.setItem("athenaeum.workspace", picked);
      setWorkspace(await api.workspacePath());
      setSelected(null);
      setBrief(null);
      await refresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function runSearch(event: React.FormEvent) {
    event.preventDefault();
    if (!workspace) return;
    try {
      setBusy(true);
      if (query.trim() === "") {
        await refresh();
      } else {
        const hits = await api.search(query, 50);
        setNodes(hits.map((hit) => hit.node));
      }
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function selectNode(id: string) {
    try {
      setSelected(await api.nodeDetail(id));
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function buildBrief() {
    if (!workspace || query.trim() === "") return;
    try {
      setBusy(true);
      setBrief(await api.assembleBrief(query, undefined, 2));
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function addNode(type: string, title: string) {
    try {
      const created = await api.createNode({ type, title, statedByUser: true });
      await refresh();
      await selectNode(created.id);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">Athenaeum</div>
        <form className="search" onSubmit={runSearch}>
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={workspace ? "Search, or describe a task…" : "Open a project first"}
            disabled={!workspace}
            aria-label="Search knowledge"
          />
          <button type="submit" disabled={!workspace || busy}>
            Search
          </button>
          <button type="button" onClick={buildBrief} disabled={!workspace || busy}>
            Brief
          </button>
        </form>
        <button type="button" className="ghost" onClick={chooseProject} disabled={busy}>
          {workspace ? "Change project" : "Open project"}
        </button>
      </header>

      {error && (
        <div className="banner error" role="alert">
          {error}
        </div>
      )}

      {!workspace ? (
        <EmptyState onOpen={chooseProject} />
      ) : (
        <main className="panes">
          <NodeList
            nodes={nodes}
            selectedId={selected?.node.id ?? null}
            onSelect={selectNode}
            onAdd={addNode}
            vocab={vocab}
          />
          <section className="pane detail">
            {brief ? (
              <BriefView brief={brief} onClose={() => setBrief(null)} />
            ) : selected ? (
              <NodeView detail={selected} onSelect={selectNode} />
            ) : (
              <p className="hint">Select a node, or search for a task and press Brief.</p>
            )}
          </section>
        </main>
      )}

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
    </div>
  );
}

function EmptyState({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="empty">
      <h1>Open a project</h1>
      <p>
        Athenaeum keeps its knowledge in an <code>.athenaeum/</code> directory beside your
        code. Pick a project folder and it will be created if it is not there yet.
      </p>
      <button type="button" onClick={onOpen}>
        Choose folder…
      </button>
    </div>
  );
}

function NodeList({
  nodes,
  selectedId,
  onSelect,
  onAdd,
  vocab,
}: {
  nodes: KnowledgeNode[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAdd: (type: string, title: string) => void;
  vocab: Vocabularies | null;
}) {
  const [title, setTitle] = useState("");
  const [type, setType] = useState("concept");

  function submit(event: React.FormEvent) {
    event.preventDefault();
    if (title.trim() === "") return;
    onAdd(type, title.trim());
    setTitle("");
  }

  return (
    <section className="pane list">
      <form className="add" onSubmit={submit}>
        <select value={type} onChange={(event) => setType(event.target.value)} aria-label="Node type">
          {(vocab?.nodeTypes ?? ["concept"]).map((option) => (
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
        <button type="submit">Add</button>
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

function NodeView({
  detail,
  onSelect,
}: {
  detail: NodeDetail;
  onSelect: (id: string) => void;
}) {
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

function BriefView({ brief, onClose }: { brief: KnowledgeBrief; onClose: () => void }) {
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
            {brief.relationships.map((relation, index) => (
              <li key={index}>
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
