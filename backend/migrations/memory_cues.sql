-- When each memory would come to mind: a few situations it was kept for
-- ("有人问周末去哪玩时"), written with it. Recall reads them with the
-- memory, by its words and by its meaning, so a question that shares no word
-- with what was said still finds it. Created by migration 004 and, for
-- databases already past it, by the startup heal `ensure_memory_cues_table`;
-- both run this same file.
CREATE TABLE IF NOT EXISTS agent_memory_cues (
    memory_id VARCHAR(64) PRIMARY KEY REFERENCES agent_memories(id) ON DELETE CASCADE,
    cues TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
