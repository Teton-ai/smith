-- Addresses a device reports per network interface, replaced on every system_info update.
-- Rows with a gateway define LAN membership (same gateway MAC + same subnet), and the
-- addresses are what future device-to-device links (WireGuard) will use as endpoints.
CREATE TABLE device_interface_address (
    device_id   INTEGER     NOT NULL REFERENCES device (id) ON DELETE CASCADE,
    interface   TEXT        NOT NULL,
    address     INET        NOT NULL,
    mac_address MACADDR,
    -- Only set when the default gateway is inside this address's subnet.
    gateway_ip  INET,
    gateway_mac MACADDR,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (device_id, interface, address)
);

CREATE INDEX device_interface_address_lan_idx
    ON device_interface_address (gateway_mac, network(address))
    WHERE gateway_mac IS NOT NULL;
