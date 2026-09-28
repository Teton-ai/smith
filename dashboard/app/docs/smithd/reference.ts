// `smithd/openapi.json` is written by `cargo test` in smithd/ from the utoipa
// annotations on the control API handlers (`smithd/src/control/server.rs`), so
// to change what `/docs/smithd` says, edit those, never this page. Bundled at
// build time: the socket it describes is only reachable on a device.
import raw from "../../../../smithd/openapi.json";
import { parseApiReference, type RawSpec } from "../api/spec";

export const smithdReference = parseApiReference(raw as RawSpec);
