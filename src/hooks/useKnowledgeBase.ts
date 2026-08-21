import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import {
  api,
  errorMessage,
  type KnowledgeBrief,
  type KnowledgeNode,
  type NodeDetail,
  type Stats,
  type Vocabularies,
} from "../api";

/** Where the last-opened project path is remembered between launches. */
const WORKSPACE_KEY = "athenaeum.workspace";

/** How many nodes a search returns before the user has to narrow it. */
const SEARCH_LIMIT = 50;

/** Hops the brief walks out from its seed nodes. */
const BRIEF_DEPTH = 2;

export interface KnowledgeBase {
  workspace: string | null;
  stats: Stats | null;
  vocab: Vocabularies | null;
  nodes: KnowledgeNode[];
  selected: NodeDetail | null;
  brief: KnowledgeBrief | null;
  query: string;
  error: string | null;
  busy: boolean;
  setQuery: (query: string) => void;
  chooseProject: () => Promise<void>;
  runSearch: () => Promise<void>;
  selectNode: (id: string) => Promise<void>;
  buildBrief: () => Promise<void>;
  closeBrief: () => void;
  addNode: (type: string, title: string) => Promise<void>;
}

/**
 * All application state and every call into the Rust side.
 *
 * Components below this take props and emit callbacks; none of them reach for
 * `api` directly, which is what keeps them renderable without a Tauri host.
 */
export function useKnowledgeBase(): KnowledgeBase {
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [vocab, setVocab] = useState<Vocabularies | null>(null);
  const [nodes, setNodes] = useState<KnowledgeNode[]>([]);
  const [selected, setSelected] = useState<NodeDetail | null>(null);
  const [query, setQuery] = useState("");
  const [brief, setBrief] = useState<KnowledgeBrief | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Monotonic ticket for whatever is currently allowed to write `nodes`.
  // Searches and refreshes both land here, and the slower of two in flight
  // must not overwrite the newer one's results — typing a query, hitting
  // Search twice, or racing the mount-time refresh would otherwise leave the
  // list showing results for a query the user has already replaced.
  const listRequest = useRef(0);

  const loadNodes = useCallback(async (load: () => Promise<KnowledgeNode[]>) => {
    const ticket = ++listRequest.current;
    setBusy(true);
    try {
      const next = await load();
      if (ticket !== listRequest.current) return;
      setNodes(next);
      setError(null);
    } catch (err) {
      if (ticket !== listRequest.current) return;
      setError(errorMessage(err));
    } finally {
      if (ticket === listRequest.current) setBusy(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    await loadNodes(async () => {
      const [nextStats, nextNodes] = await Promise.all([api.stats(), api.listNodes()]);
      setStats(nextStats);
      return nextNodes;
    });
  }, [loadNodes]);

  // The vocabularies are loaded on their own, not alongside the remembered
  // project: they come from the library rather than from any knowledge base,
  // and folding them into the restore meant a moved project folder left every
  // dropdown stuck on its fallback for the rest of the session.
  useEffect(() => {
    void (async () => {
      try {
        setVocab(await api.vocabularies());
      } catch (err) {
        setError(errorMessage(err));
      }
    })();
  }, []);

  // Reopen the last project on launch, so the app does not greet a returning
  // user with an empty screen and a file picker.
  useEffect(() => {
    const remembered = localStorage.getItem(WORKSPACE_KEY);
    if (!remembered) return;

    void (async () => {
      try {
        await api.openWorkspace(remembered);
        setWorkspace(await api.workspacePath());
        await refresh();
      } catch (err) {
        // The folder moved or was deleted. Forget it rather than failing the
        // same way on every launch from here on.
        localStorage.removeItem(WORKSPACE_KEY);
        setError(errorMessage(err));
      }
    })();
  }, [refresh]);

  const chooseProject = useCallback(async () => {
    let picked: string | string[] | null;
    try {
      picked = await openDialog({ directory: true, multiple: false });
    } catch (err) {
      setError(errorMessage(err));
      return;
    }
    if (typeof picked !== "string") return;

    setBusy(true);
    try {
      await api.openWorkspace(picked);
      localStorage.setItem(WORKSPACE_KEY, picked);
      setWorkspace(await api.workspacePath());
      setSelected(null);
      setBrief(null);
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
      return;
    } finally {
      setBusy(false);
    }
    await refresh();
  }, [refresh]);

  const runSearch = useCallback(async () => {
    if (!workspace) return;
    if (query.trim() === "") {
      await refresh();
      return;
    }
    await loadNodes(async () => {
      const hits = await api.search(query, SEARCH_LIMIT);
      return hits.map((hit) => hit.node);
    });
  }, [workspace, query, refresh, loadNodes]);

  const selectNode = useCallback(async (id: string) => {
    try {
      setSelected(await api.nodeDetail(id));
      setBrief(null);
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const buildBrief = useCallback(async () => {
    if (!workspace || query.trim() === "") return;
    setBusy(true);
    try {
      setBrief(await api.assembleBrief(query, undefined, BRIEF_DEPTH));
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [workspace, query]);

  const closeBrief = useCallback(() => setBrief(null), []);

  const addNode = useCallback(
    async (type: string, title: string) => {
      setBusy(true);
      try {
        const created = await api.createNode({ type, title, statedByUser: true });
        setError(null);
        await refresh();
        await selectNode(created.id);
      } catch (err) {
        setError(errorMessage(err));
      } finally {
        setBusy(false);
      }
    },
    [refresh, selectNode],
  );

  return {
    workspace,
    stats,
    vocab,
    nodes,
    selected,
    brief,
    query,
    error,
    busy,
    setQuery,
    chooseProject,
    runSearch,
    selectNode,
    buildBrief,
    closeBrief,
    addNode,
  };
}
