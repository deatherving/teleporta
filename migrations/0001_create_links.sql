-- Source of truth for link definitions. A link is identified by its
-- normalized path (e.g. '/v/123456'). Teleporta treats the path and metadata
-- as opaque; the mobile app owns parsing and routing.
CREATE TABLE links (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  path TEXT UNIQUE NOT NULL,
  route_type TEXT NOT NULL,
  web_fallback_url TEXT,
  ios_store_url TEXT,
  android_store_url TEXT,
  metadata JSONB NOT NULL DEFAULT '{}',
  is_active BOOLEAN NOT NULL DEFAULT TRUE,
  expires_at TIMESTAMPTZ,
  created_by TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- `path` already has a UNIQUE btree index from the constraint above, which
-- serves the primary resolution lookup (WHERE path = $1).
CREATE INDEX idx_links_route_type ON links(route_type);
CREATE INDEX idx_links_is_active ON links(is_active);
