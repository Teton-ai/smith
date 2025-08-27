ALTER TABLE device ADD COLUMN IF NOT EXISTS last_ip_address INET;

CREATE TABLE IF NOT EXISTS ip_addresses (
    ip_address INET PRIMARY KEY,
    name TEXT,
    continent TEXT,
    continent_code TEXT,
    country_code TEXT,
    country TEXT,
    region TEXT,
    city TEXT,
    isp TEXT,
    mobile TEXT,
    proxy TEXT,
    hosting TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
