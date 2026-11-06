// Agent-CLI spawning (external CLI providers) is deliberately absent:
// DeepSeek Harness owns the agent runtime. See docs/porting-athenaeum.md.
pub mod ebook;
pub mod external_search;
pub mod extract_images;
pub mod file_history;
pub mod file_sync;
pub mod fs;
pub mod project;
pub mod project_maintenance;
pub mod search;
pub mod vectorstore;
