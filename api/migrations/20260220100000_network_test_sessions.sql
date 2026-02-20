create table network_test_sessions
(
    id               uuid                     default gen_random_uuid() not null
        constraint network_test_sessions_pk
            primary key,
    created_at       timestamp with time zone default now()             not null,
    label_filter     text                                               not null,
    duration_minutes int                                                not null,
    device_count     int                                                not null,
    device_set_hash  text                                               not null,
    bundle_id        uuid                                               not null
        constraint network_test_sessions_bundle_fk
            references command_bundles (uuid)
);

alter table network_test_sessions
    owner to fleetadmin;

grant select on network_test_sessions to bugbuster;
