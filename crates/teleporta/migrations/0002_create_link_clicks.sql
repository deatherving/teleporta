-- Lightweight operational click log. For debugging and abuse investigation,
-- NOT attribution. `link_id` is nullable so clicks on unknown/expired paths
-- are still recorded. Raw IP is only populated when explicitly enabled.
CREATE TABLE link_clicks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  link_id UUID REFERENCES links(id),
  clicked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  request_path TEXT NOT NULL,
  query_params JSONB NOT NULL DEFAULT '{}',
  user_agent TEXT,
  referrer TEXT,
  platform TEXT,
  destination_type TEXT,
  ip_hash TEXT,
  raw_ip INET
);

CREATE INDEX idx_link_clicks_link_id ON link_clicks(link_id);
CREATE INDEX idx_link_clicks_clicked_at ON link_clicks(clicked_at);
CREATE INDEX idx_link_clicks_ip_hash ON link_clicks(ip_hash);
CREATE INDEX idx_link_clicks_raw_ip ON link_clicks(raw_ip);
