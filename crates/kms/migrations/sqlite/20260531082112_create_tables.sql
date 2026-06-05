CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY NOT NULL,
  definition TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS nomenclatures (
  id TEXT PRIMARY KEY NOT NULL,
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  lang TEXT NOT NULL,
  full TEXT NOT NULL,
  abbr TEXT,
  UNIQUE(lang, full)
);

CREATE TABLE IF NOT EXISTS knowledges (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  knowledge_type TEXT NOT NULL,
  entities TEXT NOT NULL,
  content TEXT,
  UNIQUE(title)
);

CREATE TABLE IF NOT EXISTS indexes (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT,
  target TEXT,
  target_type TEXT NOT NULL DEFAULT 'group',
  parent_id TEXT,
  position INTEGER NOT NULL
);

CREATE TRIGGER IF NOT EXISTS indexes_cascade_delete
AFTER DELETE ON indexes
FOR EACH ROW
BEGIN
    DELETE FROM indexes WHERE parent_id = OLD.id;
END;
