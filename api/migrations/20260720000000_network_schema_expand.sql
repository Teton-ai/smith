-- Expand-only, backward-compatible schema change: adds the security axis, the
-- credentials/identity envelopes, and the reference ledger. Nothing reads these
-- columns yet; later stages backfill and start using them.

ALTER TABLE public.network
    ADD COLUMN security_type text,
    ADD COLUMN credentials   jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN identity       jsonb;

-- External reference ledger: one row per durable external hold on a network.
-- external_key is the holder's own identifier (e.g. App API's network id), which
-- may reference several smith networks, so the identity of a hold is the full
-- triple (holder, external_key, network_id). Keyed on the triple, a holder can
-- release one specific hold without touching its others, and a network shared by
-- K holds has K rows. Collection deletes a network only when it has no ledger
-- rows and no internal FK references.
CREATE TABLE public.network_reference (
    network_id   integer NOT NULL REFERENCES public.network(id) ON DELETE RESTRICT,
    holder       text NOT NULL,
    external_key text NOT NULL,
    PRIMARY KEY (holder, external_key, network_id)
);

CREATE INDEX idx_network_reference_network_id ON public.network_reference(network_id);
