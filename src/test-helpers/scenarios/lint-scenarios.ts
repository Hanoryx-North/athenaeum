import type { LintScenario } from "./types"

// Structural lint — orphans, broken links, dead ends — is no longer
// answered here. It is a projection over the knowledge index, and its
// scenarios live in `crates/athenaeum-core/src/vault/lint.rs`, where they
// run against a vault on disk rather than against a mocked file tree.
//
// What is left is semantic lint: the prompt, and the findings parsed back
// out of a model's answer. Those are this layer's, and only a canned model
// response can exercise them.

function page(title: string, body: string): string {
  return `---\ntitle: ${title}\n---\n\n# ${title}\n\n${body}\n`
}

export const lintScenarios: LintScenario[] = [
  {
    name: "semantic/contradiction-found",
    description:
      "Two cross-linked pages make conflicting claims. The mocked semantic " +
      "LLM response emits a LINT block that the parser extracts into a " +
      "contradiction finding.",
    initialWiki: {
      "wiki/attention.md": page(
        "Attention",
        "Attention ALWAYS uses the softmax function. See [[transformer]].",
      ),
      "wiki/transformer.md": page(
        "Transformer",
        "The transformer's [[attention]] layer uses a linear kernel, not softmax.",
      ),
    },
    llmResponse: [
      "Reviewing the pages I found one contradiction:",
      "",
      "---LINT: contradiction | warning | Attention function differs between pages---",
      "attention.md claims softmax is always used, but transformer.md describes a",
      "linear attention kernel. One page needs correction.",
      "PAGES: attention.md, transformer.md",
      "---END LINT---",
    ].join("\n"),
    expected: {
      semantic: [
        {
          // The parser collapses every semantic finding to type="semantic";
          // the model's own type ("contradiction") lives in the detail.
          type: "semantic",
          severity: "warning",
          titleContains: "Attention function differs",
        },
      ],
    },
  },

  {
    name: "semantic/prose-that-is-not-a-finding",
    description:
      "The model answers with an explanation and no LINT blocks at all. " +
      "Nothing is invented from prose: the run produces no findings.",
    initialWiki: {
      "wiki/attention.md": page("Attention", "Weighting tokens. See [[transformer]]."),
      "wiki/transformer.md": page("Transformer", "Built on [[attention]]."),
    },
    llmResponse:
      "I reviewed both pages carefully and found no contradictions, stale " +
      "claims or missing pages worth reporting.",
    expected: {
      semantic: [],
    },
  },
]
