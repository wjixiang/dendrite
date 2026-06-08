ALTER TABLE knowledges ADD COLUMN source_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL;
ALTER TABLE knowledges ADD COLUMN source_chunk_idx INTEGER;
