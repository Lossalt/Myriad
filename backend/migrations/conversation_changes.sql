-- Her private conversations with someone changed: a line written, edited or
-- deleted, a conversation begun, renamed, archived or deleted, or a line she
-- said unprompted. Announced on `myriad_conversations` with the person's id
-- (only the id: nothing said leaves the row), as the change commits, so a
-- live link (`conversations.live`) reads them again at once instead of
-- polling. NOTIFY folds identical payloads within one transaction, so a
-- deleted conversation with many lines is one announcement.
--
-- Used by migration 004 and by the startup heal (`ensure_conversation_changes`);
-- every statement can run again.

CREATE OR REPLACE FUNCTION agent_conversation_session_changed() RETURNS trigger AS $$
BEGIN
    IF TG_OP <> 'INSERT' AND OLD.context->>'mode' = 'chat' AND OLD.context->>'venue' IS NULL THEN
        PERFORM pg_notify('myriad_conversations', OLD.user_id::text);
    END IF;
    IF TG_OP <> 'DELETE' AND NEW.context->>'mode' = 'chat' AND NEW.context->>'venue' IS NULL THEN
        PERFORM pg_notify('myriad_conversations', NEW.user_id::text);
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION agent_conversation_message_changed() RETURNS trigger AS $$
DECLARE
    owner integer;
BEGIN
    SELECT s.user_id INTO owner FROM agent_sessions s
     WHERE s.id = CASE WHEN TG_OP = 'DELETE' THEN OLD.session_id ELSE NEW.session_id END
       AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL;
    IF owner IS NOT NULL THEN
        PERFORM pg_notify('myriad_conversations', owner::text);
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION agent_conversation_said_changed() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('myriad_conversations',
        (CASE WHEN TG_OP = 'DELETE' THEN OLD.user_id ELSE NEW.user_id END)::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DO $$
DECLARE
    wanted record;
BEGIN
    FOR wanted IN
        SELECT * FROM (VALUES
            ('agent_sessions', 'trg_agent_sessions_conversation_written',
             'AFTER INSERT OR DELETE', '', 'agent_conversation_session_changed'),
            ('agent_sessions', 'trg_agent_sessions_conversation_updated',
             'AFTER UPDATE',
             'WHEN (OLD.title IS DISTINCT FROM NEW.title OR OLD.message_count IS DISTINCT FROM NEW.message_count '
             'OR OLD.last_active_at IS DISTINCT FROM NEW.last_active_at OR OLD.archived IS DISTINCT FROM NEW.archived '
             'OR OLD.user_id IS DISTINCT FROM NEW.user_id OR OLD.context::text IS DISTINCT FROM NEW.context::text)',
             'agent_conversation_session_changed'),
            ('agent_messages', 'trg_agent_messages_conversation_written',
             'AFTER INSERT OR DELETE', '', 'agent_conversation_message_changed'),
            ('agent_messages', 'trg_agent_messages_conversation_updated',
             'AFTER UPDATE', 'WHEN (OLD.content IS DISTINCT FROM NEW.content OR OLD.role IS DISTINCT FROM NEW.role)',
             'agent_conversation_message_changed'),
            ('agent_proactive_messages', 'trg_agent_proactive_messages_conversation_written',
             'AFTER INSERT OR DELETE', '', 'agent_conversation_said_changed'),
            ('agent_proactive_messages', 'trg_agent_proactive_messages_conversation_updated',
             'AFTER UPDATE', 'WHEN (OLD.content IS DISTINCT FROM NEW.content OR OLD.say_id IS DISTINCT FROM NEW.say_id)',
             'agent_conversation_said_changed')
        ) AS t(tbl, name, timing, condition, fn)
    LOOP
        CONTINUE WHEN EXISTS (
            SELECT 1 FROM pg_trigger WHERE tgname = wanted.name AND tgrelid = to_regclass(wanted.tbl)
        );
        EXECUTE format('CREATE TRIGGER %I %s ON %I FOR EACH ROW %s EXECUTE FUNCTION %I()',
            wanted.name, wanted.timing, wanted.tbl, wanted.condition, wanted.fn);
    END LOOP;
END $$;
