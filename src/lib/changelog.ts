/**
 * Changelog shown in Settings → Changelog.
 *
 * Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
 * Modified for Athenaeum: the mechanism is upstream's, the entries are this
 * project's and start at its own first version.
 * Hardcoded rather than
 * pulled from GitHub Releases so it works offline and stays under
 * version control with the code that ships the changes.
 *
 * Conventions:
 *   - Newest version first (the UI renders in array order).
 *   - Each entry has both `en` and `zh` highlight lists; the
 *     section picks whichever matches the current i18n language.
 *   - Only user-visible changes belong here. Internal refactors,
 *     CI tweaks, and pure test work go in commit messages, not
 *     here — keep this readable for end users.
 *   - When releasing a new version: prepend a new entry with the
 *     same shape, then bump package.json / tauri.conf.json /
 *     Cargo.toml / Cargo.lock as usual.
 */

export interface ChangelogEntry {
  version: string
  date: string // YYYY-MM-DD
  highlights: {
    en: string[]
    zh: string[]
  }
}

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "0.1.0",
    date: "2026-08-22",
    highlights: {
      en: [
        "Obsidian-style interface: three-column layout, knowledge and file trees, graph view, and a markdown editor.",
        "Wiki vault on disk — raw sources, generated pages, index and log — readable by any editor.",
        "Auto wiki builder: ingest a source and get linked pages, with lint and review passes over the result.",
        "Knowledge library with typed nodes and relationships, evidence, authority and history.",
        "Knowledge briefs assembled for a task, and a versioned bridge a coding agent can ask through.",
      ],
      zh: [
        "Obsidian 风格界面：三栏布局、知识树与文件树、图谱视图和 Markdown 编辑器。",
        "磁盘上的 Wiki 仓库——原始来源、生成页面、索引与日志，任意编辑器均可读取。",
        "自动 Wiki 构建：导入来源即生成互链页面，并可对结果执行 lint 与复查。",
        "知识库支持带类型的节点与关系、证据、权威性与历史记录。",
        "面向任务组装知识简报，并提供编码智能体可查询的版本化桥接协议。",
      ],
    },
  },
]
