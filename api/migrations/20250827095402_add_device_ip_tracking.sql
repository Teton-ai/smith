CREATE TABLE IF NOT EXISTS ip_address (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    ip_address INET NOT NULL UNIQUE,
    name TEXT,
    continent TEXT,
    continent_code CHAR(2),
    country_code CHAR(2),
    country TEXT,
    region TEXT,
    city TEXT,
    isp TEXT,
    mobile BOOLEAN,
    proxy BOOLEAN,
    hosting BOOLEAN,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

ALTER TABLE device
    ADD ip_address_id INT,
    ADD CONSTRAINT fk_ip_address FOREIGN KEY (ip_address_id) REFERENCES ip_address(id);
