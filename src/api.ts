/**
 * Typed access to the Rust side.
 *
 * These types mirror `athenaeum-core`'s serde output. They are hand-written
 * rather than generated, so if the two ever disagree, the compiler here will
 * not catch it — the Rust tests are the source of truth, and this file should
 * be updated alongside any model change.
 */
import { invoke } from "@tauri-apps/api/core";

export type NodeStatus =
  | "proposed"
  | "valid"
  | "stale"
  | "stale_requires_review"
  | "superseded"
  | "contradicted"
  | "archived";

export interface KnowledgeNode {
  id: string;
  type: string;
  title: string;
  summary: string;
  content: string;
  scope: string;
  authority: string;
  status: NodeStatus;
  provenance: { source: string; detail?: string; session?: string };
  lastVerifiedAt?: string;
  createdAt: string;
  updatedAt: string;
  version: number;
}

export interface RelatedNode {
  id: string;
  title: string;
  type: string;
  edgeType: string;
  edgeId: string;
  outbound: boolean;
  established: boolean;
}

export interface Evidence {
  id: string;
  node: string;
  kind: string;
  anchor?: {
    file: string;
    symbol?: string;
    lineStart?: number;
    lineEnd?: number;
    commit?: string;
  };
  excerpt?: string;
  note?: string;
  createdAt: string;
}

export interface NodeVersion {
  node: string;
  version: number;
  title: string;
  summary: string;
  content: string;
  reason?: string;
  recordedAt: string;
}

export interface NodeUtility {
  retrievals: number;
  uses: number;
  citations: number;
  verifications: number;
  rejections: number;
}

export interface NodeDetail {
  node: KnowledgeNode;
  relationships: RelatedNode[];
  evidence: Evidence[];
  history: NodeVersion[];
  utility: NodeUtility;
}

export interface Stats {
  nodes: number;
  liveNodes: number;
  edges: number;
  evidence: number;
  events: number;
  schemaVersion: number;
}

export interface SearchHit {
  node: KnowledgeNode;
  score: number;
}

export interface BriefNode {
  id: string;
  title: string;
  type: string;
  summary: string;
  authority: string;
  status: NodeStatus;
  depth: number;
}

export interface KnowledgeBrief {
  task: string;
  nodes: BriefNode[];
  relationships: { from: string; type: string; to: string }[];
  anchors: string[];
  warnings: string[];
  truncated: boolean;
}

export interface Vocabularies {
  nodeTypes: string[];
  edgeTypes: string[];
  authorities: string[];
  scopes: string[];
  evidenceKinds: string[];
  modelRoles: string[];
}

export const api = {
  openWorkspace: (path: string) => invoke<Stats>("open_workspace", { path }),
  workspacePath: () => invoke<string | null>("workspace_path"),
  stats: () => invoke<Stats>("stats"),
  vocabularies: () => invoke<Vocabularies>("vocabularies"),

  listNodes: (options: { nodeType?: string; includeArchived?: boolean; limit?: number } = {}) =>
    invoke<KnowledgeNode[]>("list_nodes", options),
  nodeDetail: (id: string) => invoke<NodeDetail>("node_detail", { id }),
  createNode: (input: {
    type: string;
    title: string;
    summary?: string;
    content?: string;
    authority?: string;
    scope?: string;
    statedByUser?: boolean;
  }) => invoke<KnowledgeNode>("create_node", { input }),
  updateNode: (input: {
    id: string;
    title: string;
    summary: string;
    content: string;
    reason?: string;
  }) => invoke<KnowledgeNode>("update_node", { input }),
  setNodeStatus: (id: string, status: NodeStatus) =>
    invoke<KnowledgeNode>("set_node_status", { id, status }),
  deleteNode: (id: string) => invoke<void>("delete_node", { id }),

  linkNodes: (from: string, edgeType: string, to: string, note?: string) =>
    invoke<unknown>("link_nodes", { from, edgeType, to, note }),
  promoteEdge: (id: string) => invoke<unknown>("promote_edge", { id }),
  removeEdge: (id: string) => invoke<void>("remove_edge", { id }),

  attachEvidence: (args: {
    node: string;
    file: string;
    symbol?: string;
    excerpt?: string;
    kind?: string;
  }) => invoke<Evidence>("attach_evidence", args),

  search: (query: string, limit?: number) => invoke<SearchHit[]>("search", { query, limit }),
  assembleBrief: (task: string, seeds?: string[], depth?: number) =>
    invoke<KnowledgeBrief>("assemble_brief", { task, seeds, depth }),
};

/** Tauri returns command errors as plain strings; surface them as such. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
