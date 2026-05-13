ALTER TABLE device ALTER COLUMN ip_address_id TYPE bigint;
ALTER TABLE ip_address ALTER COLUMN id TYPE bigint;
ALTER SEQUENCE ip_address_id_seq AS bigint;
