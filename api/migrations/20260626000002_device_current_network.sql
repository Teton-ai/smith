ALTER TABLE device ADD COLUMN current_network_id INTEGER REFERENCES network(id);
