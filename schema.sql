--
-- PostgreSQL database dump
--

\restrict ILQiXFSOykgwARJ0IKvGPbLlWn7cf1WHatgM9LU8xn1vTYhOqCAHRnNiS0ar8pp

-- Dumped from database version 15.15 (Debian 15.15-1.pgdg13+1)
-- Dumped by pg_dump version 15.15 (Debian 15.15-1.pgdg13+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: public; Type: SCHEMA; Schema: -; Owner: fleetadmin
--

CREATE SCHEMA public;


ALTER SCHEMA public OWNER TO fleetadmin;

--
-- Name: SCHEMA public; Type: COMMENT; Schema: -; Owner: fleetadmin
--

COMMENT ON SCHEMA public IS 'standard public schema';


--
-- Name: deployment_status; Type: TYPE; Schema: public; Owner: fleetadmin
--

CREATE TYPE public.deployment_status AS ENUM (
    'in_progress',
    'failed',
    'canceled',
    'done'
);


ALTER TYPE public.deployment_status OWNER TO fleetadmin;

--
-- Name: network_type; Type: TYPE; Schema: public; Owner: fleetadmin
--

CREATE TYPE public.network_type AS ENUM (
    'wifi',
    'ethernet',
    'dongle'
);


ALTER TYPE public.network_type OWNER TO fleetadmin;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


ALTER TABLE public._sqlx_migrations OWNER TO fleetadmin;

--
-- Name: command_queue; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.command_queue (
    id integer NOT NULL,
    device_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    cmd json NOT NULL,
    continue_on_error boolean NOT NULL,
    canceled boolean DEFAULT false NOT NULL,
    fetched boolean DEFAULT false NOT NULL,
    fetched_at timestamp with time zone,
    bundle uuid NOT NULL
);


ALTER TABLE public.command_queue OWNER TO fleetadmin;

--
-- Name: command2_queue_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.command2_queue_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.command2_queue_id_seq OWNER TO fleetadmin;

--
-- Name: command2_queue_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.command2_queue_id_seq OWNED BY public.command_queue.id;


--
-- Name: command_response; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.command_response (
    id integer NOT NULL,
    device_id integer NOT NULL,
    command_id integer,
    response json NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    status integer DEFAULT 0 NOT NULL
);


ALTER TABLE public.command_response OWNER TO fleetadmin;

--
-- Name: command2_response_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.command2_response_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.command2_response_id_seq OWNER TO fleetadmin;

--
-- Name: command2_response_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.command2_response_id_seq OWNED BY public.command_response.id;


--
-- Name: command_bundles; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.command_bundles (
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    created_on timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.command_bundles OWNER TO fleetadmin;

--
-- Name: deployment; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.deployment (
    id integer NOT NULL,
    release_id integer NOT NULL,
    status public.deployment_status NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.deployment OWNER TO fleetadmin;

--
-- Name: deployment_devices; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.deployment_devices (
    id integer NOT NULL,
    deployment_id integer NOT NULL,
    device_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.deployment_devices OWNER TO fleetadmin;

--
-- Name: deployment_devices_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.deployment_devices ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.deployment_devices_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: deployment_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.deployment ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.deployment_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: device; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.device (
    id integer NOT NULL,
    serial_number text NOT NULL,
    wifi_mac text,
    created_on timestamp with time zone DEFAULT now() NOT NULL,
    modified_on timestamp with time zone DEFAULT now() NOT NULL,
    last_ping timestamp with time zone,
    note text,
    approved boolean DEFAULT false NOT NULL,
    token text,
    release_id integer,
    target_release_id integer,
    system_info jsonb,
    network_id integer,
    modem_id integer,
    archived boolean DEFAULT false NOT NULL,
    ip_address_id integer
);


ALTER TABLE public.device OWNER TO fleetadmin;

--
-- Name: device_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.device_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.device_id_seq OWNER TO fleetadmin;

--
-- Name: device_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.device_id_seq OWNED BY public.device.id;


--
-- Name: device_label; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.device_label (
    device_id integer NOT NULL,
    label_id integer NOT NULL,
    value character varying(255) NOT NULL
);


ALTER TABLE public.device_label OWNER TO postgres;

--
-- Name: device_network; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.device_network (
    device_id integer NOT NULL,
    network_score integer,
    source text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    download_speed_mbps double precision,
    upload_speed_mbps double precision,
    CONSTRAINT device_network_network_score_check CHECK (((network_score >= 1) AND (network_score <= 5)))
);


ALTER TABLE public.device_network OWNER TO fleetadmin;

--
-- Name: device_release_upgrades; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.device_release_upgrades (
    id integer NOT NULL,
    device_id integer NOT NULL,
    previous_release_id integer NOT NULL,
    upgraded_release_id integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.device_release_upgrades OWNER TO fleetadmin;

--
-- Name: device_release_upgrades_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.device_release_upgrades ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.device_release_upgrades_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: release_new_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.release_new_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.release_new_id_seq OWNER TO fleetadmin;

--
-- Name: distribution; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.distribution (
    id integer DEFAULT nextval('public.release_new_id_seq'::regclass) NOT NULL,
    name text NOT NULL,
    description text,
    architecture text DEFAULT 'arm64'::text NOT NULL
);


ALTER TABLE public.distribution OWNER TO fleetadmin;

--
-- Name: release; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.release (
    id integer NOT NULL,
    distribution_id integer NOT NULL,
    version text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    draft boolean DEFAULT true NOT NULL,
    yanked boolean DEFAULT false NOT NULL,
    user_id integer
);


ALTER TABLE public.release OWNER TO fleetadmin;

--
-- Name: distribution_release_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.distribution_release_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.distribution_release_seq OWNER TO fleetadmin;

--
-- Name: distribution_release_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.distribution_release_seq OWNED BY public.release.id;


--
-- Name: ip_address; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.ip_address (
    id integer NOT NULL,
    ip_address inet NOT NULL,
    name text,
    continent text,
    continent_code character(2),
    country_code character(2),
    country text,
    region text,
    city text,
    isp text,
    coordinates point,
    proxy boolean,
    hosting boolean,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.ip_address OWNER TO fleetadmin;

--
-- Name: ip_address_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.ip_address ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.ip_address_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: label; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.label (
    id integer NOT NULL,
    name character varying(255) NOT NULL
);


ALTER TABLE public.label OWNER TO postgres;

--
-- Name: label_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.label_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.label_id_seq OWNER TO postgres;

--
-- Name: label_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.label_id_seq OWNED BY public.label.id;


--
-- Name: ledger; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.ledger (
    id integer NOT NULL,
    device_id integer NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now(),
    class text,
    text text
);


ALTER TABLE public.ledger OWNER TO fleetadmin;

--
-- Name: ledger_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.ledger_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.ledger_id_seq OWNER TO fleetadmin;

--
-- Name: ledger_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.ledger_id_seq OWNED BY public.ledger.id;


--
-- Name: modem; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.modem (
    id integer NOT NULL,
    imei text NOT NULL,
    network_provider text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.modem OWNER TO fleetadmin;

--
-- Name: modem_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.modem ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.modem_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: network; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.network (
    id integer NOT NULL,
    network_type public.network_type NOT NULL,
    is_network_hidden boolean NOT NULL,
    ssid text,
    name text NOT NULL,
    description text,
    password text,
    CONSTRAINT network_check CHECK (((network_type <> 'wifi'::public.network_type) OR (ssid IS NOT NULL)))
);


ALTER TABLE public.network OWNER TO fleetadmin;

--
-- Name: network_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

ALTER TABLE public.network ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.network_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: package2_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.package2_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.package2_id_seq OWNER TO fleetadmin;

--
-- Name: package; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.package (
    id integer DEFAULT nextval('public.package2_id_seq'::regclass) NOT NULL,
    name text NOT NULL,
    version text NOT NULL,
    file text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    architecture text DEFAULT 'arm64'::text NOT NULL
);


ALTER TABLE public.package OWNER TO fleetadmin;

--
-- Name: release_packages; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.release_packages (
    package_id integer NOT NULL,
    release_id integer NOT NULL
);


ALTER TABLE public.release_packages OWNER TO fleetadmin;

--
-- Name: tag; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.tag (
    id integer NOT NULL,
    name text NOT NULL,
    color text
);


ALTER TABLE public.tag OWNER TO fleetadmin;

--
-- Name: tag_device; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.tag_device (
    tag_id integer NOT NULL,
    device_id integer NOT NULL
);


ALTER TABLE public.tag_device OWNER TO fleetadmin;

--
-- Name: tag_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.tag_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.tag_id_seq OWNER TO fleetadmin;

--
-- Name: tag_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.tag_id_seq OWNED BY public.tag.id;


--
-- Name: variable; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.variable (
    name text NOT NULL,
    value text NOT NULL,
    created_on timestamp with time zone DEFAULT now() NOT NULL,
    modified_on timestamp with time zone DEFAULT now() NOT NULL,
    device integer NOT NULL,
    id integer NOT NULL
);


ALTER TABLE public.variable OWNER TO fleetadmin;

--
-- Name: variable_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.variable_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.variable_id_seq OWNER TO fleetadmin;

--
-- Name: variable_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.variable_id_seq OWNED BY public.variable.id;


--
-- Name: variable_preset; Type: TABLE; Schema: public; Owner: fleetadmin
--

CREATE TABLE public.variable_preset (
    id integer NOT NULL,
    title text NOT NULL,
    description text,
    variables jsonb NOT NULL,
    created_on timestamp with time zone DEFAULT now() NOT NULL,
    modified_on timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.variable_preset OWNER TO fleetadmin;

--
-- Name: variable_preset_id_seq; Type: SEQUENCE; Schema: public; Owner: fleetadmin
--

CREATE SEQUENCE public.variable_preset_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.variable_preset_id_seq OWNER TO fleetadmin;

--
-- Name: variable_preset_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: fleetadmin
--

ALTER SEQUENCE public.variable_preset_id_seq OWNED BY public.variable_preset.id;


--
-- Name: command_queue id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_queue ALTER COLUMN id SET DEFAULT nextval('public.command2_queue_id_seq'::regclass);


--
-- Name: command_response id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_response ALTER COLUMN id SET DEFAULT nextval('public.command2_response_id_seq'::regclass);


--
-- Name: device id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device ALTER COLUMN id SET DEFAULT nextval('public.device_id_seq'::regclass);


--
-- Name: label id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.label ALTER COLUMN id SET DEFAULT nextval('public.label_id_seq'::regclass);


--
-- Name: ledger id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.ledger ALTER COLUMN id SET DEFAULT nextval('public.ledger_id_seq'::regclass);


--
-- Name: release id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release ALTER COLUMN id SET DEFAULT nextval('public.distribution_release_seq'::regclass);


--
-- Name: tag id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag ALTER COLUMN id SET DEFAULT nextval('public.tag_id_seq'::regclass);


--
-- Name: variable id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.variable ALTER COLUMN id SET DEFAULT nextval('public.variable_id_seq'::regclass);


--
-- Name: variable_preset id; Type: DEFAULT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.variable_preset ALTER COLUMN id SET DEFAULT nextval('public.variable_preset_id_seq'::regclass);


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: command_queue command2_queue_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_queue
    ADD CONSTRAINT command2_queue_pkey PRIMARY KEY (id);


--
-- Name: command_response command2_response_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_response
    ADD CONSTRAINT command2_response_pkey PRIMARY KEY (id);


--
-- Name: command_bundles command_bundles_pk; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_bundles
    ADD CONSTRAINT command_bundles_pk PRIMARY KEY (uuid);


--
-- Name: deployment_devices deployment_devices_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment_devices
    ADD CONSTRAINT deployment_devices_pkey PRIMARY KEY (id);


--
-- Name: deployment deployment_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment
    ADD CONSTRAINT deployment_pkey PRIMARY KEY (id);


--
-- Name: deployment deployment_release_id_key; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment
    ADD CONSTRAINT deployment_release_id_key UNIQUE (release_id);


--
-- Name: device_label device_label_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.device_label
    ADD CONSTRAINT device_label_pkey PRIMARY KEY (device_id, label_id);


--
-- Name: device device_modem_id_unique; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT device_modem_id_unique UNIQUE (modem_id);


--
-- Name: device_network device_network_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_network
    ADD CONSTRAINT device_network_pkey PRIMARY KEY (device_id);


--
-- Name: device device_pk; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT device_pk PRIMARY KEY (id);


--
-- Name: device_release_upgrades device_release_upgrades_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_release_upgrades
    ADD CONSTRAINT device_release_upgrades_pkey PRIMARY KEY (id);


--
-- Name: ip_address ip_address_ip_address_key; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.ip_address
    ADD CONSTRAINT ip_address_ip_address_key UNIQUE (ip_address);


--
-- Name: ip_address ip_address_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.ip_address
    ADD CONSTRAINT ip_address_pkey PRIMARY KEY (id);


--
-- Name: label label_name_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.label
    ADD CONSTRAINT label_name_key UNIQUE (name);


--
-- Name: label label_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.label
    ADD CONSTRAINT label_pkey PRIMARY KEY (id);


--
-- Name: ledger ledger_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.ledger
    ADD CONSTRAINT ledger_pkey PRIMARY KEY (id, device_id);


--
-- Name: modem modem_imei_key; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.modem
    ADD CONSTRAINT modem_imei_key UNIQUE (imei);


--
-- Name: modem modem_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.modem
    ADD CONSTRAINT modem_pkey PRIMARY KEY (id);


--
-- Name: network network_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.network
    ADD CONSTRAINT network_pkey PRIMARY KEY (id);


--
-- Name: package package2_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.package
    ADD CONSTRAINT package2_pkey PRIMARY KEY (id);


--
-- Name: distribution release2_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.distribution
    ADD CONSTRAINT release2_pkey PRIMARY KEY (id);


--
-- Name: release release_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release
    ADD CONSTRAINT release_pkey PRIMARY KEY (id);


--
-- Name: device serial_number_k; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT serial_number_k UNIQUE (serial_number);


--
-- Name: tag_device tag_device_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag_device
    ADD CONSTRAINT tag_device_pkey PRIMARY KEY (tag_id, device_id);


--
-- Name: tag tag_pk; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag
    ADD CONSTRAINT tag_pk UNIQUE (name);


--
-- Name: tag tag_pkey; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag
    ADD CONSTRAINT tag_pkey PRIMARY KEY (id);


--
-- Name: variable variable_pk; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.variable
    ADD CONSTRAINT variable_pk PRIMARY KEY (id);


--
-- Name: variable_preset variable_preset_pk; Type: CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.variable_preset
    ADD CONSTRAINT variable_preset_pk PRIMARY KEY (id);


--
-- Name: command_bundles_created_on_index; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE INDEX command_bundles_created_on_index ON public.command_bundles USING btree (created_on DESC);


--
-- Name: idx_command2_queue_device_id; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE INDEX idx_command2_queue_device_id ON public.command_queue USING btree (device_id);


--
-- Name: idx_command2_response_command_id; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE INDEX idx_command2_response_command_id ON public.command_response USING btree (command_id);


--
-- Name: idx_command2_response_device_id; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE INDEX idx_command2_response_device_id ON public.command_response USING btree (device_id);


--
-- Name: idx_device_token; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE INDEX idx_device_token ON public.device USING btree (token);


--
-- Name: uniq_device_variablename; Type: INDEX; Schema: public; Owner: fleetadmin
--

CREATE UNIQUE INDEX uniq_device_variablename ON public.variable USING btree (device, name);


--
-- Name: command_queue command2_queue_command_bundles_uuid_fk; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_queue
    ADD CONSTRAINT command2_queue_command_bundles_uuid_fk FOREIGN KEY (bundle) REFERENCES public.command_bundles(uuid);


--
-- Name: command_queue command2_queue_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_queue
    ADD CONSTRAINT command2_queue_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: command_response command2_response_command_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_response
    ADD CONSTRAINT command2_response_command_id_fkey FOREIGN KEY (command_id) REFERENCES public.command_queue(id);


--
-- Name: command_response command2_response_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.command_response
    ADD CONSTRAINT command2_response_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: deployment_devices deployment_devices_deployment_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment_devices
    ADD CONSTRAINT deployment_devices_deployment_id_fkey FOREIGN KEY (deployment_id) REFERENCES public.deployment(id);


--
-- Name: deployment_devices deployment_devices_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment_devices
    ADD CONSTRAINT deployment_devices_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: deployment deployment_release_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.deployment
    ADD CONSTRAINT deployment_release_id_fkey FOREIGN KEY (release_id) REFERENCES public.release(id);


--
-- Name: device_label device_label_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.device_label
    ADD CONSTRAINT device_label_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: device_label device_label_label_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.device_label
    ADD CONSTRAINT device_label_label_id_fkey FOREIGN KEY (label_id) REFERENCES public.label(id);


--
-- Name: device_network device_network_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_network
    ADD CONSTRAINT device_network_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id) ON DELETE CASCADE;


--
-- Name: device_release_upgrades device_release_upgrades_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_release_upgrades
    ADD CONSTRAINT device_release_upgrades_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: device_release_upgrades device_release_upgrades_previous_release_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_release_upgrades
    ADD CONSTRAINT device_release_upgrades_previous_release_id_fkey FOREIGN KEY (previous_release_id) REFERENCES public.release(id);


--
-- Name: device_release_upgrades device_release_upgrades_upgraded_release_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device_release_upgrades
    ADD CONSTRAINT device_release_upgrades_upgraded_release_id_fkey FOREIGN KEY (upgraded_release_id) REFERENCES public.release(id);


--
-- Name: device fk_ip_address; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT fk_ip_address FOREIGN KEY (ip_address_id) REFERENCES public.ip_address(id);


--
-- Name: device fk_modem; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT fk_modem FOREIGN KEY (modem_id) REFERENCES public.modem(id);


--
-- Name: device fk_network_id; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT fk_network_id FOREIGN KEY (network_id) REFERENCES public.network(id);


--
-- Name: device fk_release_id; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT fk_release_id FOREIGN KEY (release_id) REFERENCES public.release(id);


--
-- Name: release_packages fk_release_id; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release_packages
    ADD CONSTRAINT fk_release_id FOREIGN KEY (release_id) REFERENCES public.release(id);


--
-- Name: device fk_target_release_id; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.device
    ADD CONSTRAINT fk_target_release_id FOREIGN KEY (target_release_id) REFERENCES public.release(id);


--
-- Name: ledger ledger_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.ledger
    ADD CONSTRAINT ledger_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: release release_distribution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release
    ADD CONSTRAINT release_distribution_id_fkey FOREIGN KEY (distribution_id) REFERENCES public.distribution(id);


--
-- Name: release_packages release_packages2_package_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release_packages
    ADD CONSTRAINT release_packages2_package_id_fkey FOREIGN KEY (package_id) REFERENCES public.package(id);


--
-- Name: release release_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.release
    ADD CONSTRAINT release_user_id_fkey FOREIGN KEY (user_id) REFERENCES auth.users(id) ON DELETE SET NULL;


--
-- Name: tag_device tag_device_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag_device
    ADD CONSTRAINT tag_device_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.device(id);


--
-- Name: tag_device tag_device_tag_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.tag_device
    ADD CONSTRAINT tag_device_tag_id_fkey FOREIGN KEY (tag_id) REFERENCES public.tag(id);


--
-- Name: variable variable_device_null_fk; Type: FK CONSTRAINT; Schema: public; Owner: fleetadmin
--

ALTER TABLE ONLY public.variable
    ADD CONSTRAINT variable_device_null_fk FOREIGN KEY (device) REFERENCES public.device(id) ON UPDATE CASCADE ON DELETE RESTRICT;


--
-- PostgreSQL database dump complete
--

\unrestrict ILQiXFSOykgwARJ0IKvGPbLlWn7cf1WHatgM9LU8xn1vTYhOqCAHRnNiS0ar8pp

