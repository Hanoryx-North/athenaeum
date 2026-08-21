import { BriefView } from "./components/BriefView";
import { EmptyState } from "./components/EmptyState";
import { NodeList } from "./components/NodeList";
import { NodeView } from "./components/NodeView";
import { StatusBar } from "./components/StatusBar";
import { TopBar } from "./components/TopBar";
import { useKnowledgeBase } from "./hooks/useKnowledgeBase";

/**
 * The application shell.
 *
 * Composition only: state and every call into Rust live in
 * {@link useKnowledgeBase}, and everything visible is a component.
 *
 * Deliberately not a graph visualization (buildplan §33): the graph has to
 * hold something worth looking at before drawing it is the useful work. What
 * this gives you is the part that matters first — read, write, search,
 * navigate, and see the brief an agent would receive.
 */
export default function App() {
  const kb = useKnowledgeBase();

  return (
    <div className="app">
      <TopBar
        query={kb.query}
        hasWorkspace={kb.workspace !== null}
        busy={kb.busy}
        onQueryChange={kb.setQuery}
        onSearch={kb.runSearch}
        onBrief={kb.buildBrief}
        onChooseProject={kb.chooseProject}
      />

      {kb.error && (
        <div className="banner error" role="alert">
          {kb.error}
        </div>
      )}

      {kb.workspace === null ? (
        <EmptyState onOpen={kb.chooseProject} busy={kb.busy} />
      ) : (
        <main className="panes">
          <NodeList
            nodes={kb.nodes}
            selectedId={kb.selected?.node.id ?? null}
            vocab={kb.vocab}
            busy={kb.busy}
            onSelect={kb.selectNode}
            onAdd={kb.addNode}
          />
          <section className="pane detail">
            {kb.brief ? (
              <BriefView brief={kb.brief} onClose={kb.closeBrief} />
            ) : kb.selected ? (
              <NodeView detail={kb.selected} onSelect={kb.selectNode} />
            ) : (
              <p className="hint">Select a node, or search for a task and press Brief.</p>
            )}
          </section>
        </main>
      )}

      <StatusBar stats={kb.stats} workspace={kb.workspace} />
    </div>
  );
}
